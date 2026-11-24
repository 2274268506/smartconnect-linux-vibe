//! Observe GL-window closure even when a damage-driven phone stream sends no new frames.

use gst::glib::translate::ToGlibPtr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn watch(window: &gst_gl::GLWindow, closed: &Arc<AtomicBool>) {
    unsafe extern "C" fn on_close(data: gst::glib::ffi::gpointer) {
        // The GL thread only posts a flag; the host closes the session on its own main loop.
        let flag = unsafe { &*(data as *const Arc<AtomicBool>) };
        flag.store(true, Ordering::Release);
    }
    unsafe extern "C" fn destroy(data: gst::glib::ffi::gpointer) {
        drop(unsafe { Box::from_raw(data as *mut Arc<AtomicBool>) });
    }
    let data = Box::into_raw(Box::new(closed.clone()));
    // SAFETY: GLWindow owns this callback/data until replacement or finalization, then invokes
    // destroy exactly once. The owned Arc stays valid on the GL thread and cannot cycle to GTK
    // or a pipeline. Replacing the sink's frame-driven callback is intentional: the host takes
    // responsibility for notifying the phone and asynchronously setting the pipeline to NULL.
    unsafe {
        gst_gl::ffi::gst_gl_window_set_close_callback(
            window.to_glib_none().0,
            Some(on_close),
            data.cast(),
            Some(destroy),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gst::prelude::*;
    use gst_gl::prelude::*;
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};

    #[test]
    #[ignore = "requires an X11/XWayland GL session; run with GST_GL_WINDOW=x11"]
    fn idle_video_window_close_is_observed_without_another_frame() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "appsrc name=input is-live=true format=time \
            caps=video/x-raw,format=RGBA,width=320,height=240,framerate=0/1 \
            ! glimagesink name=video sync=false",
        )
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        pipeline.set_state(gst::State::Playing).unwrap();
        let mut buffer = gst::Buffer::with_size(320 * 240 * 4).unwrap();
        buffer.get_mut().unwrap().set_pts(gst::ClockTime::ZERO);
        assert_eq!(
            pipeline
                .by_name("input")
                .unwrap()
                .emit_by_name::<gst::FlowReturn>("push-buffer", &[&buffer]),
            gst::FlowReturn::Ok
        );
        let sink = pipeline.by_name("video").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let window = loop {
            let context = sink.property::<Option<gst_gl::GLContext>>("context");
            let rendered = sink
                .clone()
                .downcast::<gst::Bin>()
                .unwrap()
                .iterate_recurse()
                .into_iter()
                .filter_map(Result::ok)
                .filter(|child| child.find_property("stats").is_some())
                .any(|child| {
                    child
                        .property::<gst::Structure>("stats")
                        .get::<u64>("rendered")
                        .unwrap_or(0)
                        > 0
                });
            if rendered {
                if let Some(window) = context.and_then(|context| context.window()) {
                    break window;
                }
            }
            assert!(Instant::now() < deadline, "GL window never appeared");
            std::thread::sleep(Duration::from_millis(10));
        };
        let closed = Arc::new(AtomicBool::new(false));
        watch(&window, &closed);
        let handle = unsafe { gst_gl::ffi::gst_gl_window_get_window_handle(window.to_glib_none().0) as u32 };
        let (connection, _) = x11rb::connect(None).unwrap();
        let protocols = connection
            .intern_atom(false, b"WM_PROTOCOLS")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let delete = connection
            .intern_atom(false, b"WM_DELETE_WINDOW")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let event = ClientMessageEvent::new(32, handle, protocols, [delete, x11rb::CURRENT_TIME, 0, 0, 0]);
        connection
            .send_event(false, handle, EventMask::NO_EVENT, event)
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !closed.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let observed = closed.load(Ordering::Acquire);
        pipeline.set_state(gst::State::Null).unwrap();
        assert!(observed, "close waited for a second video frame");
    }
}
