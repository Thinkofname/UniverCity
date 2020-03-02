//! A image loader that loads images in the background whilst
//! initially returning a usable template if possible

use std::mem;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::errors;
use crate::prelude::*;
use crate::server::assets;
use png;

/// Loads png assets from the asset manager
pub enum Loader {}

type FutureData = (assets::ResourceKey<'static>, Mutex<FutureInner>, Condvar);

struct ImgInfo {
    reader: png::Reader<assets::Asset>,
    info: png::OutputInfo,
}

pub struct LoaderData {
    log: Logger,
    task_send: mpsc::Sender<(ImgInfo, Arc<FutureData>)>,
}

macro_rules! try_task {
    ($task:expr, $e:expr) => {
        try_task!($task, $e, {
            continue;
        })
    };
    ($task:expr, $e:expr, $ret:expr) => {
        match { $e } {
            Ok(val) => val,
            Err(err) => {
                {
                    let mut inner = $task.1.lock().expect("Failed to get task lock");
                    inner.state = State::Error(err.into());
                    $task.2.notify_all();
                }
                $ret;
            }
        }
    };
}

impl Loader {
    fn thread(recv: mpsc::Receiver<(ImgInfo, Arc<FutureData>)>) {
        while let Ok((ImgInfo { mut reader, info }, task)) = recv.recv() {
            let mut buf = vec![0; info.buffer_size()];
            try_task!(task, reader.next_frame(&mut buf));
            let buf = match info.color_type {
                png::ColorType::RGBA => buf,
                png::ColorType::RGB => {
                    let mut out = vec![0u8; (info.width * info.height * 4) as usize];
                    for (cin, cou) in buf.chunks_exact(3).zip(out.chunks_exact_mut(4)) {
                        cou[0] = cin[0];
                        cou[1] = cin[1];
                        cou[2] = cin[2];
                        cou[3] = 255;
                    }
                    out
                }
                png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => {
                    let mut out = vec![0u8; (info.width * info.height * 4) as usize];
                    let alpha = info.color_type == png::ColorType::GrayscaleAlpha;
                    let chunks = if alpha { 2 } else { 1 };
                    for (cin, cou) in buf.chunks_exact(chunks).zip(out.chunks_exact_mut(4)) {
                        cou[0] = cin[0];
                        cou[1] = cin[0];
                        cou[2] = cin[0];
                        cou[3] = if alpha { cin[1] } else { 255 };
                    }
                    out
                }
                png::ColorType::Indexed => unreachable!(),
            };
            {
                let mut inner = task.1.lock().expect("Failed to lock task");
                inner.state = State::Done(Image {
                    width: info.width,
                    height: info.height,
                    data: buf,
                });
                task.2.notify_all();
            }
        }
    }
}

impl<'a> assets::AssetLoader<'a> for Loader {
    type LoaderData = LoaderData;
    type Key = assets::ResourceKey<'a>;
    type Return = ImageFuture;

    fn init(assets: &assets::Store) -> Self::LoaderData {
        let (send, recv) = mpsc::channel();
        thread::spawn(move || Loader::thread(recv));
        LoaderData {
            log: assets.log.new(o!("source" => "image_loader")),
            task_send: send,
        }
    }

    fn load(
        data: &mut Self::LoaderData,
        assets: &assets::AssetManager,
        key: assets::ResourceKey<'_>,
    ) -> server::UResult<Self::Return> {
        let task = Arc::new((
            key.borrow().into_owned(),
            Mutex::new(FutureInner {
                state: State::Loading,
            }),
            Condvar::new(),
        ));
        let ret = ImageFuture { inner: task };

        // Read the header here
        let file = try_task!(
            ret.inner,
            assets.open_from_pack(
                key.module_key(),
                &format!("textures/{}.png", key.resource())
            ),
            return Ok(ret)
        );
        let decoder = png::Decoder::new(file);
        let (info, reader) = try_task!(ret.inner, decoder.read_info(), return Ok(ret));
        {
            let mut inner = ret.inner.1.lock().expect("Failed to lock task");
            inner.state = State::Header(info.width, info.height);
            ret.inner.2.notify_all();
        }

        assume!(
            data.log,
            data.task_send
                .send((ImgInfo { reader, info }, ret.inner.clone()))
        );
        Ok(ret)
    }
}

enum State {
    Loading,
    Header(u32, u32),
    Done(Image),
    Taken(u32, u32),
    Error(errors::Error),
    // After the error has been taken
    Broken,
}

/// A image that may not be currently loader but could be
/// in the future.
pub struct ImageFuture {
    inner: Arc<FutureData>,
}

struct FutureInner {
    state: State,
}

impl ImageFuture {
    /// Returns a completed image future
    pub fn completed(width: u32, height: u32, data: Vec<u8>) -> ImageFuture {
        let task = Arc::new((
            ResourceKey::new("fake", "fake"),
            Mutex::new(FutureInner {
                state: State::Done(Image {
                    width,
                    height,
                    data,
                }),
            }),
            Condvar::new(),
        ));
        ImageFuture { inner: task }
    }

    /// Returns the resource key that this image is being loaded
    /// from.
    pub fn key(&self) -> assets::ResourceKey<'_> {
        self.inner.0.borrow()
    }

    /// Returns the requested image's dimensions if it
    /// loaded the required information.
    #[allow(dead_code)]
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        let data = self.inner.1.lock().expect("Failed to lock task");
        match data.state {
            State::Header(w, h) | State::Taken(w, h) => Some((w, h)),
            State::Done(ref img) => Some((img.width, img.height)),
            _ => None,
        }
    }

    /// Blocks until the dimensions of the image have been loaded
    /// or an error occurs.
    pub fn wait_dimensions(&self) -> Option<(u32, u32)> {
        let mut data = self.inner.1.lock().expect("Failed to lock task");
        loop {
            match mem::replace(&mut data.state, State::Broken) {
                State::Header(w, h) => {
                    data.state = State::Header(w, h);
                    return Some((w, h));
                }
                State::Taken(w, h) => {
                    data.state = State::Taken(w, h);
                    return Some((w, h));
                }
                State::Done(img) => {
                    let dims = (img.width, img.height);
                    data.state = State::Done(img);
                    return Some(dims);
                }
                State::Error(err) => {
                    data.state = State::Error(err);
                    return None;
                }
                State::Broken => panic!("Already errored"),
                orig => {
                    data.state = orig;
                }
            }
            data = self.inner.2.wait(data).expect("Failed to lock task");
        }
    }

    /// Returns the loaded image if it has been loaded.
    ///
    /// This will take the image preventing it from taken again.
    pub fn take_image(&self) -> Option<Image> {
        let mut data = self.inner.1.lock().expect("Failed to lock task");
        match mem::replace(&mut data.state, State::Broken) {
            State::Done(img) => {
                data.state = State::Taken(img.width, img.height);
                Some(img)
            }
            orig => {
                data.state = orig;
                None
            }
        }
    }

    /// Blocks until the image is loaded or an error occurs.
    ///
    /// This will take the image preventing it from taken again.
    #[allow(dead_code)]
    pub fn wait_take_image(&self) -> errors::Result<Image> {
        let mut data = self.inner.1.lock().expect("Failed to lock task");
        loop {
            match mem::replace(&mut data.state, State::Broken) {
                State::Done(img) => {
                    data.state = State::Taken(img.width, img.height);
                    return Ok(img);
                }
                State::Taken(_, _) => panic!("Image already taken"),
                State::Error(err) => return Err(err),
                State::Broken => panic!("Already errored"),
                orig => {
                    data.state = orig;
                }
            }
            data = self.inner.2.wait(data).expect("Failed to lock task");
        }
    }

    /// Returns a `Result::Err` if the decoding/loading of the image failed
    pub fn error(&self) -> errors::Result<()> {
        let mut data = self.inner.1.lock().expect("Failed to lock task");
        match mem::replace(&mut data.state, State::Broken) {
            State::Error(err) => Err(err),
            State::Broken => panic!("Already errored"),
            orig => {
                data.state = orig;
                Ok(())
            }
        }
    }
}

/// A loaded image in rgba format
pub struct Image {
    /// Width of the image in pixels
    pub width: u32,
    /// Height of the image in pixels
    pub height: u32,
    /// Raw RGBA data of the image
    pub data: Vec<u8>,
}
