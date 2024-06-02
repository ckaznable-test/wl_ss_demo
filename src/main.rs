use std::os::fd::AsFd;

use image::{ImageBuffer, Rgba};
use memmap2::{MmapMut, MmapOptions};
use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer::WlBuffer,
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
    },
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};

fn main() -> anyhow::Result<()> {
    let conn = Connection::connect_to_env().unwrap();
    let display = conn.display();
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut state = State::new();
    while state.running {
        queue.blocking_dispatch(&mut state).unwrap();
    }

    Ok(())
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore ZwlrScreencopyManagerV1);

#[derive(Default)]
struct State {
    running: bool,
    mmap: Option<MmapMut>,
    buffer: Option<WlBuffer>,
    screencopy_man: Option<ZwlrScreencopyManagerV1>,
}

impl State {
    fn new() -> Self {
        Self {
            running: true,
            ..Default::default()
        }
    }

    fn save_image(&self) {
        if let Some(mmap) = &self.mmap {
            let rgba_data: Vec<u8> = mmap[..]
                .chunks(4)
                .flat_map(|chunk| vec![chunk[2], chunk[1], chunk[0], 0xFF])
                .collect();

            let width = 1920;
            let height = 1080;
            let img_buffer: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba_data).unwrap();
            img_buffer.save("output.png").unwrap();

            println!("Image saved as output.png");
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match &interface[..] {
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, version, qh, ());
                    let (init_w, init_h) = (1920, 1080);
                    let stride = init_w * 4;
                    let size = stride * init_h;

                    let file = tempfile::tempfile().unwrap();
                    file.set_len(size as u64).unwrap();

                    let pool = shm.create_pool(file.as_fd(), size, qh, ());
                    let buffer = pool.create_buffer(
                        0,
                        init_w,
                        init_h,
                        stride,
                        wl_shm::Format::Xrgb8888,
                        qh,
                        (),
                    );

                    state.buffer = Some(buffer.clone());
                    state.mmap = Some(unsafe { MmapOptions::new().map_mut(&file).unwrap() });
                }
                "zwlr_screencopy_manager_v1" => {
                    state.screencopy_man = Some(registry
                        .bind::<ZwlrScreencopyManagerV1, _, _>( name, version, qh, ()));
                }
                "wl_output" => {
                    let _output = registry
                        .bind::<WlOutput, _, _>(name, version, qh, ());
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event::*;
        match event {
            Ready {..} => {
                proxy.destroy();
                state.running = false;
                state.save_image();
            }
            BufferDone => {
                if let Some(buf) = state.buffer.clone() {
                    proxy.copy(&buf)
                }
            }
            _ => {}
        }
    }
}


impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Done = event {
            let Some(man) = &state.screencopy_man else {
                return;
            };

            man.capture_output(0, output, qh, ());
        }
    }
}
