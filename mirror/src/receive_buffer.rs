//! Give libnice's UDP sockets room for high-resolution video bursts without system-wide changes.
use std::collections::HashMap;

use gio::prelude::*;
use gst::glib::{self, translate::*};
use gst::prelude::*;

const RECEIVE_BYTES: i32 = 4 * 1024 * 1024;

#[link(name = "nice")]
unsafe extern "C" {
    fn nice_agent_get_sockets(
        agent: *mut glib::gobject_ffi::GObject,
        stream: u32,
        component: u32,
    ) -> *mut glib::ffi::GPtrArray;
}

#[derive(Default)]
pub struct ReceiveBuffers {
    sockets: HashMap<usize, i32>,
}

impl ReceiveBuffers {
    pub fn sizes(&self) -> Vec<i32> {
        self.sockets.values().copied().collect()
    }

    fn tune(&mut self, socket: gio::Socket) {
        if socket.socket_type() != gio::SocketType::Datagram {
            return;
        }
        let before = socket
            .option(libc::SOL_SOCKET, libc::SO_RCVBUF)
            .unwrap_or(0);
        let key = socket.as_ptr() as usize;
        if self.sockets.get(&key) == Some(&before) {
            return;
        }
        let result = if before >= RECEIVE_BYTES * 2 {
            Ok(())
        } else {
            socket.set_option(libc::SOL_SOCKET, libc::SO_RCVBUF, RECEIVE_BYTES)
        };
        let actual = socket
            .option(libc::SOL_SOCKET, libc::SO_RCVBUF)
            .unwrap_or(0);
        if let Err(error) = result {
            eprintln!("video UDP receive buffer could not be enlarged: {error}");
        } else {
            println!("video UDP receive buffer: requested={RECEIVE_BYTES} actual={actual} bytes");
        }
        self.sockets.insert(key, actual);
    }

    pub fn update(&mut self, pipeline: &gst::Pipeline) {
        for element in pipeline
            .iterate_recurse()
            .into_iter()
            .filter_map(Result::ok)
        {
            if element
                .factory()
                .is_none_or(|factory| factory.name() != "nicesrc")
            {
                continue;
            }
            let Some(agent) = element.property::<Option<glib::Object>>("agent") else {
                continue;
            };
            let stream = element.property::<u32>("stream");
            let component = element.property::<u32>("component");
            if stream == 0 || component == 0 {
                continue;
            }
            // SAFETY: nicesrc's agent is a NiceAgent. The API returns an owned array of GSocket
            // references. Clone each reference before releasing the array; never borrow raw FDs
            // that libnice can close/reuse on another thread. No candidate addresses are logged.
            unsafe {
                let array = nice_agent_get_sockets(agent.to_glib_none().0, stream, component);
                if array.is_null() {
                    continue;
                }
                for index in 0..(*array).len as usize {
                    let socket: gio::Socket =
                        from_glib_none((*array).pdata.add(index).read().cast());
                    self.tune(socket);
                }
                glib::ffi::g_ptr_array_unref(array);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "nice")]
    unsafe extern "C" {
        fn nice_agent_new(
            context: *mut glib::ffi::GMainContext,
            compatibility: i32,
        ) -> *mut glib::gobject_ffi::GObject;
        fn nice_agent_add_stream(agent: *mut glib::gobject_ffi::GObject, components: u32) -> u32;
        fn nice_agent_gather_candidates(
            agent: *mut glib::gobject_ffi::GObject,
            stream: u32,
        ) -> glib::ffi::gboolean;
    }

    #[test]
    fn libnice_gathered_sockets_are_tuned_through_the_receive_element() {
        gst::init().unwrap();
        let context = glib::MainContext::new();
        // RFC5245 compatibility is 0. Owned NiceAgent plus a live MainContext outlive the source.
        let agent: glib::Object =
            unsafe { from_glib_full(nice_agent_new(context.to_glib_none().0, 0)) };
        let stream = unsafe { nice_agent_add_stream(agent.to_glib_none().0, 1) };
        assert!(stream > 0);
        assert_ne!(
            unsafe { nice_agent_gather_candidates(agent.to_glib_none().0, stream) },
            0
        );
        let source = gst::ElementFactory::make("nicesrc")
            .property("agent", &agent)
            .property("stream", stream)
            .property("component", 1u32)
            .build()
            .unwrap();
        let pipeline = gst::Pipeline::new();
        pipeline.add(&source).unwrap();
        let mut buffers = ReceiveBuffers::default();
        buffers.update(&pipeline);
        let sizes = buffers.sizes();
        assert!(!sizes.is_empty(), "must discover actual libnice sockets");
        assert!(sizes.iter().all(|size| *size > 0));
        buffers.update(&pipeline);
        assert_eq!(buffers.sizes().len(), sizes.len());
    }

    #[test]
    fn udp_receive_buffer_grows_and_repeated_updates_do_not_retune() {
        let socket = gio::Socket::new(
            gio::SocketFamily::Ipv4,
            gio::SocketType::Datagram,
            gio::SocketProtocol::Udp,
        )
        .unwrap();
        let before = socket.option(libc::SOL_SOCKET, libc::SO_RCVBUF).unwrap();
        let mut buffers = ReceiveBuffers::default();
        buffers.tune(socket.clone());
        let after = socket.option(libc::SOL_SOCKET, libc::SO_RCVBUF).unwrap();
        assert!(after >= before, "buffer must not shrink");
        assert!(after > 0);
        // Linux caps unprivileged requests at rmem_max; respect the host's policy in CI.
        let limit = std::fs::read_to_string("/proc/sys/net/core/rmem_max")
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        assert!(after >= before.max(2 * RECEIVE_BYTES.min(limit)));
        buffers.tune(socket);
        assert_eq!(buffers.sizes(), &[after]);
    }
}
