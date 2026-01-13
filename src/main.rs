// Check if 'winit' is a direct dependency and adjust use statements if necessary.
// Assuming 'winit' and 'pixels' are in Cargo.toml:
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use pixels::{Pixels, SurfaceTexture};   

fn main() {
    // 1. Setup Winit window
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("My Pixels Window") // Added a title for clarity
        .build(&event_loop)
        .unwrap();

    // 2. Setup Pixels buffer - dynamically get window size
    let mut pixels = {
        let window_size = window.inner_size();
        // Use the actual window dimensions for the Pixels buffer
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        Pixels::new(window_size.width, window_size.height, surface_texture)
            .expect("Failed to create Pixels buffer") // Using expect for better error message
    };

    // 3. The Event Loop
    event_loop.run(move |event, el, control_flow| {
        // Set the event loop's control flow to poll for events when not busy
        // This is important for smooth rendering and responsiveness.
        el.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,

            // Handle window resizing
            Event::WindowEvent {
                event: WindowEvent::Resized(new_size),
                ..
            } => {
                // Reconfigure the surface texture and Pixels buffer
                pixels.resize_surface(new_size.width, new_size.height);
                pixels.resize_buffer(new_size.width, new_size.height);
                window.request_redraw(); // Request a redraw after resizing
            }

            Event::MainEventsCleared => {
                // Update game logic here...
                // For this example, we'll just request a redraw continuously
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                // 4. Draw to the pixel buffer
                let frame = pixels.get_frame_mut();

                // Fill the frame with red pixels
                for pixel in frame.chunks_exact_mut(4) {
                    // RGBA: Red, Green, Blue, Alpha
                    pixel[0] = 255; // Red
                    pixel[1] = 0;   // Green
                    pixel[2] = 0;   // Blue
                    pixel[3] = 255; // Alpha (fully opaque)
                }

                // Render the frame to the window
                pixels.render().expect("Failed to render pixels");
            }
            _ => (),
        }
});
}
