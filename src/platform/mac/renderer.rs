use super::metal_renderer;
use crate::platform::wgpu::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};
use gpui::{DevicePixels, GpuSpecs, PlatformAtlas, Scene, Size};
use metal::CAMetalLayer;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle,
};
use std::{
    cell::RefCell,
    ffi::c_void,
    ptr::{self, NonNull},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone)]
pub(crate) struct Context {
    metal: metal_renderer::Context,
    wgpu: GpuContext,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            metal: metal_renderer::Context::default(),
            wgpu: Rc::new(RefCell::new(None)),
        }
    }
}

pub(crate) enum Renderer {
    Metal(metal_renderer::Renderer),
    Wgpu(WgpuRenderer),
}

#[derive(Clone, Copy, Debug)]
struct RawMacWindow {
    view: *mut c_void,
}

unsafe impl Send for RawMacWindow {}
unsafe impl Sync for RawMacWindow {}

impl HasWindowHandle for RawMacWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let view = NonNull::new(self.view).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(view);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }
}

impl HasDisplayHandle for RawMacWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(handle.into()) })
    }
}

pub(crate) unsafe fn new_renderer(
    context: Context,
    native_window: *mut c_void,
    native_view: *mut c_void,
    bounds: gpui::Size<f32>,
    transparent: bool,
) -> Renderer {
    if should_use_wgpu() {
        match new_wgpu_renderer(&context, native_view, bounds, transparent) {
            Ok(renderer) => {
                log::info!("using macOS wgpu renderer");
                return Renderer::Wgpu(renderer);
            }
            Err(error) => {
                log::error!(
                    "failed to create macOS wgpu renderer, falling back to Metal: {error:#}"
                );
            }
        }
    }

    Renderer::Metal(unsafe {
        metal_renderer::new_renderer(
            context.metal,
            native_window,
            native_view,
            bounds,
            transparent,
        )
    })
}

fn should_use_wgpu() -> bool {
    !matches!(
        std::env::var("GPUI_MAC_RENDERER").as_deref(),
        Ok("metal") | Ok("Metal") | Ok("METAL")
    )
}

fn new_wgpu_renderer(
    context: &Context,
    native_view: *mut c_void,
    bounds: gpui::Size<f32>,
    transparent: bool,
) -> anyhow::Result<WgpuRenderer> {
    let raw_window = RawMacWindow { view: native_view };
    let config = WgpuSurfaceConfig {
        size: Size {
            width: DevicePixels(bounds.width.ceil().max(1.0) as i32),
            height: DevicePixels(bounds.height.ceil().max(1.0) as i32),
        },
        transparent,
    };

    WgpuRenderer::new(Rc::clone(&context.wgpu), &raw_window, config, None)
}

impl Renderer {
    pub fn layer(&self) -> Option<&metal::MetalLayerRef> {
        match self {
            Self::Metal(renderer) => renderer.layer(),
            Self::Wgpu(_) => None,
        }
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        match self {
            Self::Metal(renderer) => renderer.layer_ptr(),
            Self::Wgpu(_) => ptr::null_mut(),
        }
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        if let Self::Metal(renderer) = self {
            renderer.set_presents_with_transaction(presents_with_transaction);
        }
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        match self {
            Self::Metal(renderer) => renderer.update_drawable_size(size),
            Self::Wgpu(renderer) => renderer.update_drawable_size(size),
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        match self {
            Self::Metal(renderer) => renderer.update_transparency(transparent),
            Self::Wgpu(renderer) => renderer.update_transparency(transparent),
        }
    }

    pub fn destroy(&mut self) {
        match self {
            Self::Metal(renderer) => renderer.destroy(),
            Self::Wgpu(renderer) => renderer.destroy(),
        }
    }

    pub fn draw(&mut self, scene: &Scene) {
        match self {
            Self::Metal(renderer) => renderer.draw(scene),
            Self::Wgpu(renderer) => renderer.draw(scene),
        }
    }

    pub fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        match self {
            Self::Metal(renderer) => renderer.sprite_atlas().clone(),
            Self::Wgpu(renderer) => renderer.sprite_atlas().clone(),
        }
    }

    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        match self {
            Self::Metal(_) => None,
            Self::Wgpu(renderer) => Some(renderer.gpu_specs()),
        }
    }

    pub fn wgpu_device_queue(&self) -> Option<gpui::WgpuDeviceQueue> {
        match self {
            Self::Metal(_) => None,
            Self::Wgpu(renderer) => Some(renderer.device_queue()),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> anyhow::Result<image::RgbaImage> {
        match self {
            Self::Metal(renderer) => renderer.render_to_image(scene),
            Self::Wgpu(_) => anyhow::bail!("render_to_image is not implemented for macOS wgpu"),
        }
    }
}
