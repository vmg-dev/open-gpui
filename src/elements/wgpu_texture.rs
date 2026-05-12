use crate::{
    App, Bounds, DevicePixels, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, ObjectFit, Pixels, Size, Style, StyleRefinement, Styled,
    WgpuTextureAlphaMode, Window,
};
use refineable::Refineable;
use std::sync::Arc;

/// A source of a wgpu texture component's content.
#[derive(Clone, Debug)]
pub struct WgpuTextureSource {
    texture: Arc<::wgpu::TextureView>,
    _owner: Option<Arc<::wgpu::Texture>>,
    size: Size<DevicePixels>,
}

impl WgpuTextureSource {
    /// Create a source from an existing [`wgpu::TextureView`] and its device-pixel size.
    pub fn new(texture: Arc<::wgpu::TextureView>, size: Size<DevicePixels>) -> Self {
        Self {
            texture,
            _owner: None,
            size,
        }
    }

    /// Create a source from a [`wgpu::Texture`] and keep it alive with the view.
    pub fn from_texture(texture: Arc<::wgpu::Texture>, size: Size<DevicePixels>) -> Self {
        let view = Arc::new(texture.create_view(&Default::default()));
        Self {
            texture: view,
            _owner: Some(texture),
            size,
        }
    }
}

/// A component that paints an existing wgpu texture view directly.
pub struct WgpuTexture {
    source: WgpuTextureSource,
    object_fit: ObjectFit,
    alpha_mode: WgpuTextureAlphaMode,
    style: StyleRefinement,
}

/// Create a new wgpu texture element.
pub fn wgpu_texture(source: WgpuTextureSource) -> WgpuTexture {
    WgpuTexture {
        source,
        object_fit: ObjectFit::Contain,
        alpha_mode: WgpuTextureAlphaMode::Blend,
        style: Default::default(),
    }
}

impl WgpuTexture {
    /// Set the object fit for the texture.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }

    /// Treat the texture as fully opaque when compositing.
    pub fn opaque(mut self) -> Self {
        self.alpha_mode = WgpuTextureAlphaMode::Opaque;
        self
    }

    /// Set how the texture's alpha channel is composited.
    pub fn alpha_mode(mut self, alpha_mode: WgpuTextureAlphaMode) -> Self {
        self.alpha_mode = alpha_mode;
        self
    }
}

impl Element for WgpuTexture {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let bounds = self.object_fit.get_bounds(bounds, self.source.size);
        window.paint_wgpu_texture_with_alpha_mode(
            bounds,
            Arc::clone(&self.source.texture),
            self.alpha_mode,
        );
    }
}

impl IntoElement for WgpuTexture {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for WgpuTexture {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
