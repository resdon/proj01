use::winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use pixels::{Pixels, SurfaceTexture};   

// 1. Setup Winit window
let event_loop = EventLoop::new();
let window = WindowBuilder::new().build(&event_loop).unwrap();

// 2. Setup Pixels buffer
let mut pixels = {
    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    Pixels::new(640, 480, surface_texture).unwrap()
};

// 3. The Event Loop
event_loop.run(move |event, _, control_flow| {
    match event {
        Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
        Event::MainEventsCleared => {
            // Update game logic here...
            window.request_redraw();
        }
        Event::RedrawRequested(_) => {
            // 4. Draw to the pixel buffer
            let frame = pixels.get_frame_mut();
            for pixel in frame.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[0xff, 0x00, 0x00, 0xff]); // Fill screen with Red
            }
            pixels.render().unwrap();
        }
        _ => (),
    }
});
