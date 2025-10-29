use crate::core::gui::Gui;
use egui_wgpu::Renderer as EguiRenderer;
use objc2::{msg_send, sel};
use objc2::runtime::Object;
use objc2_foundation::CoreGraphics::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::CAMetalLayer;
use once_cell::sync::OnceCell;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, UiKitDisplayHandle, UiKitWindowHandle, WindowHandle,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    egui_renderer: EguiRenderer,
    surface_config: wgpu::SurfaceConfiguration,
}

struct RawWindowHandleWrapper {
    window_handle: RawWindowHandle,
    display_handle: RawDisplayHandle,
}

impl HasWindowHandle for RawWindowHandleWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.window_handle) })
    }
}
impl HasDisplayHandle for RawWindowHandleWrapper {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.display_handle) })
    }
}

static RENDERER: OnceCell<Mutex<Renderer>> = OnceCell::new();
pub struct IosGui;

pub fn init() {
    info!("Initializing GUI...");
    super::hook::setup_render_hook();
    super::input_hook::init();
}

pub fn render_frame(gui: &mut Gui, drawable: *mut c_void) {
    let renderer = RENDERER.get_or_init(|| {
        info!("First frame detected. Initializing renderer...");
        let renderer = unsafe { Renderer::new(drawable as *mut Object) };
        Mutex::new(renderer)
    });
    let mut renderer = renderer.lock().unwrap();

    renderer.render(gui);
}

impl Renderer {
    unsafe fn new(drawable: *mut Object) -> Self {
        let layer: *mut CAMetalLayer = msg_send![drawable, layer];

        let view: *mut Object = msg_send![layer as *mut Object, delegate];
        let view_controller: *mut Object = msg_send![view, nextResponder];

        let ui_view = NonNull::new(view as *mut c_void).expect("UIView pointer was null");
        let ui_view_controller = NonNull::new(view_controller as *mut c_void);

        let mut window_handle = UiKitWindowHandle::new(ui_view);
        window_handle.ui_view_controller = ui_view_controller;
        let raw_window_handle = RawWindowHandle::UiKit(window_handle);

        let display_handle = UiKitDisplayHandle::new();
        let raw_display_handle = RawDisplayHandle::UiKit(display_handle);

        let handle_wrapper = RawWindowHandleWrapper {
            window_handle: raw_window_handle,
            display_handle: raw_display_handle,
        };

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });

        let surface = instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&handle_wrapper).unwrap())
            .expect("Failed to create wgpu surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })).expect("Failed to find compatible wgpu adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor::default(),
            None,
        )).expect("Failed to get wgpu device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter().copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let frame: CGRect = msg_send![view, frame];
        let size: CGSize = frame.size;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width as u32,
            height: size.height as u32,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let egui_renderer = EguiRenderer::new(&device, surface_format, None, 1);

        Self {
            surface,
            device,
            queue,
            egui_renderer,
            surface_config,
        }
    }

    fn render(&mut self, gui: &mut Gui) {
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::OutOfMemory) => {
                error!("Surface out of memory, skipping frame");
                return;
            }
            Err(e) => {
                error!("Surface error: {:?}, reconfiguring", e);
                self.surface.configure(&self.device, &self.surface_config);
                return;
            }
        };

        let texture_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let full_output = gui.run(); 

        let clipped_primitives = gui.context.tessellate(full_output.shapes, gui.context.pixels_per_point());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Egui Encoder") });
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: gui.context.pixels_per_point(),
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.egui_renderer.update_buffers(&self.device, &self.queue, &mut encoder, &clipped_primitives, &screen_descriptor);

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations { 
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store 
                    },
                })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
            });
            self.egui_renderer.render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }
        self.queue.submit(Some(encoder.finish()));
        output.present();
    }
}