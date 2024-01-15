use std::sync::{Arc, Mutex};

use color_eyre::eyre::Context;
use color_eyre::Result;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    registry_handlers,
};

use crate::surface::Surface;
use crate::wallpaper_config::WallpaperConfig;

pub struct Wpaperd {
    pub compositor_state: CompositorState,
    pub output_state: OutputState,
    pub shm_state: Shm,
    pub layer_state: LayerShell,
    pub registry_state: RegistryState,
    pub surfaces: Vec<Surface>,
    wallpaper_config: Arc<Mutex<WallpaperConfig>>,
    use_scaled_window: bool,
    egl_display: egl::Display,
}

impl Wpaperd {
    pub fn new(
        qh: &QueueHandle<Self>,
        globals: &GlobalList,
        _conn: &Connection,
        wallpaper_config: Arc<Mutex<WallpaperConfig>>,
        use_scaled_window: bool,
        egl_display: egl::Display,
    ) -> Result<Self> {
        let shm_state = Shm::bind(globals, qh)?;

        Ok(Self {
            compositor_state: CompositorState::bind(globals, qh)?,
            output_state: OutputState::new(globals, qh),
            shm_state,
            layer_state: LayerShell::bind(globals, qh)?,
            registry_state: RegistryState::new(globals),
            surfaces: Vec::new(),
            wallpaper_config,
            use_scaled_window,
            egl_display,
        })
    }

    pub fn reload_config(&mut self) -> Result<()> {
        let mut wallpaper_config = self.wallpaper_config.lock().unwrap();
        let new_config =
            WallpaperConfig::new_from_path(&wallpaper_config.path).with_context(|| {
                format!(
                    "reading configuration from file {:?}",
                    wallpaper_config.path
                )
            });
        match new_config {
            Ok(config) => {
                if !(*wallpaper_config == config) {
                    *wallpaper_config = config;
                    log::info!("Configuration updated");
                }
                Ok(())
            }
            Err(err) => {
                log::error!("{:?}", err);
                Err(err)
            }
        }
    }
}

impl CompositorHandler for Wpaperd {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let surface = self
            .surfaces
            .iter_mut()
            .enumerate()
            .find(|(_, s)| surface == &s.surface)
            .unwrap()
            .1;

        // Ignore unnecessary updates
        if surface.scale != new_factor {
            surface.scale = new_factor;
            surface.surface.set_buffer_scale(new_factor);
            surface.resize(None);
        }
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_transform: wl_output::Transform,
    ) {
        let surface = self
            .surfaces
            .iter_mut()
            .enumerate()
            .find(|(_, s)| surface == &s.surface)
            .unwrap()
            .1;

        surface.surface.set_buffer_transform(new_transform);
    }
}

impl OutputHandler for Wpaperd {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        // TODO: Error handling
        let surface = self.compositor_state.create_surface(qh);

        let info = self.output_state.info(&output).unwrap();
        let scale = if self.use_scaled_window {
            1
        } else {
            info.scale_factor
        };
        surface.set_buffer_scale(scale);

        let name = info.name.as_ref().unwrap().to_string();

        let layer = self.layer_state.create_layer_surface(
            qh,
            surface.clone(),
            Layer::Background,
            Some(format!("wpaperd-{}", name)),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);

        let empty_region = Region::new(&self.compositor_state).unwrap();
        // Wayland clients are expected to render the cursor on their input region. By setting the
        // input region to an empty region, the compositor renders the default cursor. Without
        // this, and empty desktop won't render a cursor.
        surface.set_input_region(Some(empty_region.wl_region()));

        // From `wl_surface::set_opaque_region`:
        // > Setting the pending opaque region has copy semantics, and the
        // > wl_region object can be destroyed immediately.
        empty_region.wl_region().destroy();

        let wallpaper_info = self
            .wallpaper_config
            .lock()
            .unwrap()
            .get_output_by_name(&name);

        self.surfaces.push(Surface::new(
            name,
            layer,
            output,
            surface,
            info.scale_factor,
            wallpaper_info,
            self.egl_display,
        ));
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        // TODO: Do we need to do something here?
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        // Find the destroyed output and remove it
        self.surfaces.swap_remove(
            self.surfaces
                .iter()
                .enumerate()
                .find(|(_, surface)| surface.output == output)
                .unwrap()
                .0,
        );
    }
}

impl LayerShellHandler for Wpaperd {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let surface = self
            .surfaces
            .iter_mut()
            .find(|surface| &surface.layer == layer)
            // We always know the surface that it is being configured
            .unwrap();

        if surface.dimensions != configure.new_size {
            // Update dimensions
            surface.resize(Some(configure));
        }

        surface.configured = true;
    }
}

impl ShmHandler for Wpaperd {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

delegate_compositor!(Wpaperd);
delegate_output!(Wpaperd);
delegate_shm!(Wpaperd);
delegate_registry!(Wpaperd);
delegate_layer!(Wpaperd);

impl ProvidesRegistryState for Wpaperd {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
