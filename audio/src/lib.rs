
use std::io::{Read, Seek};
use std::sync::{Arc, Mutex, MutexGuard};
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::time;
use lewton::inside_ogg::OggStreamReader;

pub trait AudioDataSource {
    fn next(&mut self) -> Option<(i16, i16)>;
    fn sample_rate(&self) -> u32;

    fn resampled(self, output: u32) -> ResampleStream<Self>
        where Self: Sized
    {
        ResampleStream::new(self, output)
    }

    fn pitched(self, rate: f32) -> ResampleStream<Self>
        where Self: Sized
    {
        ResampleStream::pitch(self, rate)
    }

    fn volume(self, volume: f32) -> VolumeStream<Self>
        where Self: Sized
    {
        VolumeStream::new(self, volume, volume)
    }

    fn volume_sides(self, left: f32, right: f32) -> VolumeStream<Self>
        where Self: Sized
    {
        VolumeStream::new(self, left, right)
    }

    fn mix<A>(self, other: A) -> MixStream<Self, A>
        where Self: Sized,
              A: AudioDataSource
    {
        MixStream::new(self, other)
    }

    fn into_buffer(mut self) -> AudioBuffer
        where Self: Sized
    {
        let sample_rate = self.sample_rate();
        let mut data = Vec::new();
        while let Some(p) = self.next() {
            data.push(p);
        }
        AudioBuffer {
            data: Arc::new(BufferData {
                data,
                sample_rate,
            }),
        }
    }

    fn set_volume_sides(&mut self, left: f32, right: f32);
}

impl AudioDataSource for Box<dyn AudioDataSource> {
    fn sample_rate(&self) -> u32 {
        (**self).sample_rate()
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        (**self).next()
    }

    fn set_volume_sides(&mut self, left: f32, right: f32) {
        (**self).set_volume_sides(left, right)
    }
}

#[derive(Clone)]
pub struct AudioBuffer {
    data: Arc<BufferData>,
}

struct BufferData {
    data: Vec<(i16, i16)>,
    sample_rate: u32,
}

impl AudioBuffer {
    pub fn source(&self) -> BufferedSource {
        BufferedSource {
            buffer: self.data.clone(),
            offset: 0,
        }
    }
}

pub struct BufferedSource {
    buffer: Arc<BufferData>,
    offset: usize,
}

impl AudioDataSource for BufferedSource {
    fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        let data = self.buffer.data.get(self.offset).cloned();
        self.offset += 1;
        data
    }

    fn set_volume_sides(&mut self, _left: f32, _right: f32) {

    }
}

#[derive(Clone)]
pub struct AudioMixer {
    sample_rate: u32,
    data: Arc<Mutex<AudioMixerData>>,
}

impl AudioMixer {
    pub fn new(sample_rate: u32) -> AudioMixer {
        AudioMixer {
            sample_rate,
            data: Arc::new(Mutex::new(AudioMixerData {
                last_time: time::Instant::now(),
                sounds: Vec::new(),
            })),
        }
    }

    pub fn tick(&self) -> MutexGuard<AudioMixerData> {
        let mut data = self.data.lock().unwrap();
        let diff = data.last_time.elapsed();
        let delta = (diff.as_secs() * 1_000_000_000 + u64::from(diff.subsec_nanos())) as f32 / 1_000_000_000.0;
        data.last_time = time::Instant::now();
        for sound in &mut data.sounds {
            if sound.time_to_play > 0.0 {
                sound.time_to_play -= delta;
            }
        }
        data.sounds.retain(|v| !v.shared.ended.load(Ordering::Relaxed));
        data
    }

    pub fn play<A>(&self, audio: A) -> SoundRef
        where A: AudioDataSource + Send + 'static
    {
        self.play_later(audio, 0.0)
    }

    pub fn play_later<A>(&self, audio: A, delay: f32) -> SoundRef
        where A: AudioDataSource + Send + 'static
    {
        let mut data = self.data.lock().unwrap();
        assert_eq!(self.sample_rate, audio.sample_rate());

        let shared = Arc::new(SoundShared {
            paused: AtomicBool::new(true),
            ended: AtomicBool::new(false),
            volume: Mutex::new(None),
        });

        data.sounds.push(Sound {
            data: Box::new(audio),
            time_to_play: delay,
            shared: shared.clone(),
        });

        SoundRef {
            shared,
        }
    }
}

struct Sound {
    data: Box<dyn AudioDataSource + Send>,
    time_to_play: f32,
    shared: Arc<SoundShared>,
}

struct SoundShared {
    paused: AtomicBool,
    ended: AtomicBool,
    volume: Mutex<Option<(f32, f32)>>,
}

#[derive(Clone)]
pub struct SoundRef {
    shared: Arc<SoundShared>,
}

impl SoundRef {
    pub fn play(&self) {
        self.shared.paused.store(false, Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.shared.paused.store(false, Ordering::Relaxed)
    }

    pub fn has_ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.shared.ended.store(true, Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: f32) {
        *self.shared.volume.lock().unwrap() = Some((volume, volume));
    }

    pub fn set_volume_sides(&self, left: f32, right: f32) {
        *self.shared.volume.lock().unwrap() = Some((left, right));
    }
}

pub struct AudioMixerData {
    last_time: time::Instant,

    sounds: Vec<Sound>,
}

impl AudioMixerData {
    pub fn next_sample(&mut self) -> (i16, i16) {
        let mut left = 0i16;
        let mut right = 0i16;

        for sound in &mut self.sounds {
            if sound.shared.paused.load(Ordering::Relaxed) {
                continue;
            }
            let mut volume = sound.shared.volume.lock().unwrap();
            if let Some(vol) = volume.take() {
                sound.data.set_volume_sides(vol.0, vol.1);
            }
            if sound.time_to_play <= 0.0 && !sound.shared.ended.load(Ordering::Relaxed) {
                if let Some((l, r)) = sound.data.next() {
                    left = left.saturating_add(l);
                    right = right.saturating_add(r);
                } else {
                    sound.shared.ended.store(true, Ordering::Relaxed);
                }
            }
        }

        (left, right)
    }
}

pub struct MixStream<A, B> {
    a: A,
    b: B,
}

impl <A, B> MixStream<A, B>
    where A: AudioDataSource,
          B: AudioDataSource
{
    fn new(a: A, b: B) -> MixStream<A, B> {
        assert_eq!(a.sample_rate(), b.sample_rate());
        MixStream {
            a,
            b,
        }
    }
}

impl <A, B> AudioDataSource for MixStream<A, B>
    where A: AudioDataSource,
          B: AudioDataSource
{
    fn sample_rate(&self) -> u32 {
        self.a.sample_rate()
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        let (al, ar) = if let Some((l, r)) = self.a.next() {
            (l, r)
        } else {
            return None;
        };
        let (bl, br) = if let Some((l, r)) = self.b.next() {
            (l, r)
        } else {
            return None;
        };
        Some((
            al.saturating_add(bl),
            ar.saturating_add(br),
        ))
    }

    fn set_volume_sides(&mut self, left: f32, right: f32){
        self.a.set_volume_sides(left, right);
        self.b.set_volume_sides(left, right);
    }
}

pub struct ResampleStream<A> {
    inner: A,
    current: Option<((i16, i16), (i16, i16))>,
    output_rate: u32,
    current_pos: f32,
    step: f32,
    end: bool,
    force: bool,
}

impl <A> ResampleStream<A>
    where A: AudioDataSource
{
    fn new(stream: A, output: u32) -> ResampleStream<A> {
        ResampleStream {
            current: None,
            current_pos: 0.0,
            output_rate: output,
            step: stream.sample_rate() as f32 / output as f32,
            inner: stream,
            end: false,
            force: false,
        }
    }

    fn pitch(stream: A, rate: f32) -> ResampleStream<A> {
        ResampleStream {
            current: None,
            current_pos: 0.0,
            output_rate: stream.sample_rate(),
            step: rate,
            inner: stream,
            end: false,
            force: true,
        }
    }
}

impl <A> AudioDataSource for ResampleStream<A>
    where A: AudioDataSource
{
    fn sample_rate(&self) -> u32 {
        self.output_rate
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        if !self.force && self.output_rate == self.inner.sample_rate() {
            return self.inner.next();
        }
        if self.end {
            return None;
        }
        if self.current.is_none() {
            let l = self.inner.next();
            let n = self.inner.next();
            return if let (Some(l), Some(n)) = (l, n) {
                self.current = Some((l, n));
                Some(l)
            } else {
                self.end = true;
                None
            }
        }
        let (mut last, mut cur) = self.current.unwrap();
        while self.current_pos >= 1.0 {
            self.current_pos -= 1.0;
            if let Some(next) = self.inner.next() {
                self.current = Some((cur, next));
                last = cur;
                cur = next
            } else {
                self.end = true;
                return None;
            }
        }

        let output = (
            (f32::from(last.0) * (1.0 - self.current_pos) + f32::from(cur.0) * self.current_pos) as i16,
            (f32::from(last.1) * (1.0 - self.current_pos) + f32::from(cur.1) * self.current_pos) as i16,
        );
        self.current_pos += self.step;
        Some(output)
    }

    fn set_volume_sides(&mut self, left: f32, right: f32) {
        self.inner.set_volume_sides(left, right);
    }
}


pub struct VolumeStream<A> {
    inner: A,
    left: f32,
    right: f32,
}

impl <A> VolumeStream<A>
    where A: AudioDataSource
{
    fn new(stream: A, left: f32, right: f32) -> VolumeStream<A> {
        VolumeStream {
            inner: stream,
            left,
            right,
        }
    }
}

impl <A> AudioDataSource for VolumeStream<A>
    where A: AudioDataSource
{
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        if let Some((l, r)) = self.inner.next() {
            Some((
                (f32::from(l) * self.left) as i16,
                (f32::from(r) * self.right) as i16,
            ))
        } else {
            None
        }
    }

    fn set_volume_sides(&mut self, left: f32, right: f32) {
        self.inner.set_volume_sides(left, right);
        self.left = left;
        self.right = right;
    }
}

pub struct OggStream<R: Read + Seek> {
    ogg: OggStreamReader<R>,
    last_packet: Option<Vec<Vec<i16>>>,
    offset: usize,
    finished: bool,
}

impl <R> OggStream<R>
    where R: Read + Seek
{
    pub fn load(r: R) -> Result<OggStream<R>, lewton::VorbisError>
        where R: Read + Seek + 'static
    {
        let ogg = OggStreamReader::new(r)?;

        let ogg_stream = OggStream {
            ogg,
            last_packet: None,
            offset: 0,
            finished: false,
        };
        Ok(ogg_stream)
    }
}

impl <R> AudioDataSource for OggStream<R>
    where R: Read + Seek
{
    fn sample_rate(&self) -> u32 {
        self.ogg.ident_hdr.audio_sample_rate
    }

    fn next(&mut self) -> Option<(i16, i16)> {
        if self.finished {
            return None;
        }
        let next = if let Some(last) = self.last_packet.as_ref() {
            last[0].len() <= self.offset
        } else { true };
        if next {
            self.last_packet = None;
            loop {
                let data = self.ogg.read_dec_packet();
                if let Ok(val) = data {
                    if let Some(val) = val {
                        if val[0].is_empty() {
                            continue;
                        }
                        self.last_packet = Some(val);
                        self.offset = 0;
                        break;
                    } else {
                        self.finished = true;
                        return None;
                    }
                } else {
                    return None;
                }
            };
        }
        if let Some(last) = self.last_packet.as_mut() {
            let offset = self.offset;
            self.offset += 1;
            if last.len() == 1 {
                let val = last[0][offset];
                Some((val, val))
            } else {
                Some((
                    last[0][offset],
                    last[1][offset]
                ))
            }
        } else {
            self.finished = true;
            None
        }
    }

    fn set_volume_sides(&mut self, _left: f32, _right: f32) {
    }
}
