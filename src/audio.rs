//! Audio management

use crate::prelude::*;
use crate::server::lua::{self, Ref, Scope};
use cgmath;
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired};
use sdl2::AudioSubsystem;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use univercity_audio::{AudioBuffer, AudioDataSource, AudioMixer, OggStream, SoundRef};

/// Manages the audio device
pub struct AudioManager {
    _audio: AudioSubsystem,
    _device: AudioDevice<SDLAudioCallback>,
    /// The audio controller for this device
    pub controller: Rc<RefCell<AudioController>>,
    playlist: Option<String>,
}

#[inline]
fn send_sync<T: Send + Sync>(v: T) -> T {
    v
}

const FADE_TIME: Duration = Duration::from_secs(1);

impl AudioManager {
    /// Creates a new audio manager.
    ///
    /// This creates an audio device in sdl for output
    pub fn new(
        logger: &Logger,
        audio: AudioSubsystem,
        asset_manager: AssetManager,
    ) -> AudioManager {
        let mixer = AudioMixer::new(44_100);
        let mix = mixer.clone();
        let device = audio
            .open_playback(
                None,
                &AudioSpecDesired {
                    freq: Some(44_100),
                    channels: Some(2),
                    samples: None,
                },
                move |spec| {
                    assert_eq!(spec.freq, 44_100);
                    SDLAudioCallback { inner: mix }
                },
            )
            .expect("Failed to open an audio device");
        device.resume();

        AudioManager {
            _audio: audio,
            _device: device,
            controller: Rc::new(RefCell::new(send_sync(AudioController {
                log: logger.new(o!("source" => "audio_manager")),
                mixer,
                assets: asset_manager,
                loaded_sounds: FNVMap::default(),
                playing_sounds: Vec::new(),
                positioned_sounds: Vec::new(),
                music_volume: 0.5,
                sound_volume: 1.0,
                songs: Vec::new(),
                playing_song: None,
                camera: (0.0, 0.0, cgmath::Deg(0.0)),
            }))),
            playlist: None,
        }
    }

    /// Ticks playing music
    pub fn tick(&self, camera_x: f32, camera_y: f32, camera_rotation: cgmath::Deg<f32>) {
        use ogg_metadata::AudioMetadata;
        use rand::seq::SliceRandom;
        use rand::thread_rng;

        let controller: &mut AudioController = &mut *self.controller.borrow_mut();
        controller.camera = (camera_x, camera_y, camera_rotation);

        controller.playing_sounds.retain(|v| !v.has_ended());

        if let Some(song) = controller.playing_song.as_mut() {
            if let Some(remaining) = song.length.checked_sub(song.start.elapsed()) {
                if remaining <= FADE_TIME {
                    let vol = remaining.as_nanos() as f64 / FADE_TIME.as_nanos() as f64;
                    song.sound
                        .set_volume((vol * controller.music_volume) as f32);
                }
            } else {
                song.sound.stop();
            }
        }

        if controller
            .playing_song
            .as_ref()
            .map_or(true, |v| v.sound.has_ended())
        {
            let mut rng = thread_rng();
            if let Some(song) = controller.songs.choose(&mut rng) {
                let asset = assume!(
                    controller.log,
                    controller.assets.open_from_pack(
                        song.module_key(),
                        &format!("sound/{}.ogg", song.resource())
                    )
                );
                // Get length
                let meta = assume!(controller.log, ogg_metadata::read_format(asset));
                let length = match assume!(controller.log, meta.get(0)) {
                    ogg_metadata::OggFormat::Vorbis(meta) => {
                        meta.get_duration().expect("Missing audio duration")
                    }
                    _ => panic!("Unsupported ogg format"),
                };

                let asset = assume!(
                    controller.log,
                    controller.assets.open_from_pack(
                        song.module_key(),
                        &format!("sound/{}.ogg", song.resource())
                    )
                );

                let ogg = assume!(controller.log, OggStream::load(asset))
                    .resampled(44_100)
                    .volume(controller.music_volume as f32);
                let snd = controller.mixer.play(ogg);
                snd.play();
                controller.playing_song = Some(PlayingSong {
                    sound: snd,
                    length,
                    start: Instant::now(),
                });
            }
        }
        controller.update_positioned();
    }

    /// Starts playing random songs from the named playlist
    pub fn set_playlist(&mut self, list: &str) {
        use std::io::{BufRead, BufReader};
        let mut controller = self.controller.borrow_mut();
        let mut songs = Vec::new();

        if self.playlist.as_ref().map_or(false, |p| p == list) {
            return;
        }
        self.playlist = Some(list.to_string());

        let plist = format!("sound/music/{}.list", list);
        for m_key in controller.assets.get_packs() {
            let file = if let Ok(f) = controller.assets.open_from_pack(m_key.borrow(), &plist) {
                f
            } else {
                continue;
            };

            let file = BufReader::new(file);
            for line in file.lines() {
                let line = assume!(controller.log, line);
                let line = line.trim();
                // Skip empty lines/comments
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // Support cross module loading
                let s_key = LazyResourceKey::parse(line)
                    .or_module(m_key.borrow())
                    .into_owned();
                songs.push(s_key);
            }
        }
        controller.songs = songs;
        if let Some(song) = controller.playing_song.as_mut() {
            if let Some(remaining) = song.length.checked_sub(song.start.elapsed()) {
                if remaining > FADE_TIME {
                    song.length = (song.length - remaining) + FADE_TIME;
                }
            }
        }
    }

    /// Updates volume settings from the config
    pub fn update_settings(&self, config: &Config) {
        let mut controller = self.controller.borrow_mut();
        controller.music_volume = config.music_volume.get().powi(4);
        controller.sound_volume = config.sound_volume.get().powi(4);

        if let Some(snd) = controller.playing_song.as_ref() {
            snd.sound.set_volume(controller.music_volume as f32);
        }
        for snd in &controller.playing_sounds {
            snd.set_volume(controller.sound_volume as f32);
        }
        controller.update_positioned();
    }
}

/// Allows for playing of sounds
pub struct AudioController {
    log: Logger,
    mixer: AudioMixer,
    assets: AssetManager,

    music_volume: f64,
    sound_volume: f64,

    loaded_sounds: FNVMap<ResourceKey<'static>, AudioBuffer>,
    playing_sounds: Vec<SoundRef>,
    positioned_sounds: Vec<PositionedSound>,

    songs: Vec<ResourceKey<'static>>,
    playing_song: Option<PlayingSong>,

    camera: (f32, f32, cgmath::Deg<f32>),
}

struct PlayingSong {
    sound: SoundRef,
    length: Duration,
    start: Instant,
}

impl script::LuaTracked for AudioController {
    const KEY: script::NulledString = nul_str!("audio_ref");
    type Storage = Weak<RefCell<AudioController>>;
    type Output = Rc<RefCell<AudioController>>;

    fn try_convert(s: &Self::Storage) -> Option<Self::Output> {
        s.upgrade()
    }
}

struct PositionedSound {
    position: Arc<Mutex<(f32, f32)>>,
    sound: SoundRef,
    first: bool,
}

/// A reference to a positional sound
pub struct PositionRef {
    position: Arc<Mutex<(f32, f32)>>,
    log: Logger,
    sound: SoundRef,
}

impl PositionRef {
    /// Updates the position of this sound
    pub fn set_position(&self, x: f32, y: f32) {
        *assume!(self.log, self.position.lock()) = (x, y);
    }

    /// Returns the current position of this sound
    pub fn position(&self) -> (f32, f32) {
        *assume!(self.log, self.position.lock())
    }

    /// Returns whether the sound has ended
    pub fn has_ended(&self) -> bool {
        self.sound.has_ended()
    }

    /// Stops the sound
    pub fn stop(&self) {
        self.sound.stop();
    }
}

impl lua::LuaUsable for PositionRef {
    fn fields(t: &lua::TypeBuilder) {
        t.field(
            "get_position",
            lua::closure1(|_lua, this: Ref<PositionRef>| -> (f64, f64) {
                let (x, y) = this.position();
                (f64::from(x), f64::from(y))
            }),
        );
        t.field(
            "set_position",
            lua::closure3(|_lua, this: Ref<PositionRef>, x: f64, y: f64| {
                this.set_position(x as f32, y as f32);
            }),
        );
        t.field(
            "get_ended",
            lua::closure1(|_lua, this: Ref<PositionRef>| -> bool { this.sound.has_ended() }),
        );
        t.field(
            "set_ended",
            lua::closure2(|_lua, this: Ref<PositionRef>, ended: bool| {
                if ended {
                    this.sound.stop()
                }
            }),
        );
    }
}

component!(AudioController => mut World);

impl AudioController {
    /// Plays the named sound file
    pub fn play_sound(&mut self, sound: ResourceKey<'_>) {
        let snd = self.make_sound(sound);
        snd.play();
        self.playing_sounds.push(snd.clone());
    }

    fn make_sound(&mut self, sound: ResourceKey<'_>) -> SoundRef {
        if let Some(sound) = self.loaded_sounds.get(&sound).cloned() {
            let snd = self
                .mixer
                .play(sound.source().volume(self.sound_volume as f32));
            return snd;
        }
        let asset = assume!(
            self.log,
            self.assets.open_from_pack(
                sound.module_key(),
                &format!("sound/{}.ogg", sound.resource())
            )
        );
        let ogg = assume!(self.log, OggStream::load(asset))
            .resampled(44_100)
            .into_buffer();
        self.loaded_sounds.insert(sound.into_owned(), ogg.clone());
        self.mixer
            .play(ogg.source().volume(self.sound_volume as f32))
    }

    /// Plays the named sound file at the target position
    pub fn play_sound_at(&mut self, sound: ResourceKey<'_>, position: (f32, f32)) -> PositionRef {
        let snd = self.make_sound(sound);
        let position = Arc::new(Mutex::new(position));

        self.positioned_sounds.push(PositionedSound {
            sound: snd.clone(),
            position: position.clone(),
            first: true,
        });

        PositionRef {
            log: self.log.clone(),
            sound: snd,
            position,
        }
    }

    fn update_positioned(&mut self) {
        use std::f32::consts::PI;
        self.positioned_sounds.retain(|v| !v.sound.has_ended());

        let ang = -cgmath::Rad::from(self.camera.2).0 + 5.0f32.atan2(0.0) + (PI / 4.0) * 3.0;
        let c = ang.cos();
        let s = ang.sin();

        let (x1, y1) = (self.camera.0 + s, self.camera.1 + c);
        let (x2, y2) = (self.camera.0 - s, self.camera.1 - c);

        for snd in &mut self.positioned_sounds {
            let (x, y) = { *assume!(self.log, snd.position.lock()) };

            let distance = (y - self.camera.1).hypot(x - self.camera.0);

            let (left, right) = if distance > 15.0 {
                (0.0, 0.0)
            } else {
                let side = (y2 - y1) * x - (x2 - x1) * y + x2 * y1 - y2 * x1;

                let dvol = 1.0 - distance / 15.0;

                let mut vol = if side < 0.0 {
                    let am = side.abs() / 30.0;
                    (1.0 + am * 0.5, 1.0 - am * 0.5)
                } else {
                    let am = side.abs() / 30.0;
                    (1.0 - am * 0.5, 1.0 + am * 0.5)
                };
                vol.0 *= dvol;
                vol.1 *= dvol;

                vol
            };

            snd.sound.set_volume_sides(
                left * self.sound_volume as f32,
                right * self.sound_volume as f32,
            );

            if snd.first {
                snd.sound.play();
            }
        }
    }
}

struct SDLAudioCallback {
    inner: AudioMixer,
}
impl AudioCallback for SDLAudioCallback {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        let mut data = self.inner.tick();
        for out in out.chunks_exact_mut(2) {
            let data = data.next_sample();
            out[0] = data.0;
            out[1] = data.1;
        }
    }
}

/// Sets up a interface for scripts to interface with audio playback
pub fn init_audiolib(state: &lua::Lua) {
    state.set(
        Scope::Global,
        "audio_play_sound",
        lua::closure2(
            |lua, module: Ref<String>, sound: Ref<String>| -> UResult<()> {
                let audio = lua
                    .get_tracked::<AudioController>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let mut audio = audio.borrow_mut();
                let key = LazyResourceKey::parse(&sound)
                    .or_module(ModuleKey::new(&*module))
                    .into_owned();
                audio.play_sound(key);
                Ok(())
            },
        ),
    );
    state.set(
        Scope::Global,
        "audio_play_sound_at",
        lua::closure4(
            |lua,
             module: Ref<String>,
             sound: Ref<String>,
             x: f64,
             y: f64|
             -> UResult<Ref<PositionRef>> {
                let audio = lua
                    .get_tracked::<AudioController>()
                    .ok_or_else(|| ErrorKind::InvalidState)?;
                let mut audio = audio.borrow_mut();
                let key = LazyResourceKey::parse(&sound)
                    .or_module(ModuleKey::new(&*module))
                    .into_owned();
                Ok(Ref::new(
                    lua,
                    audio.play_sound_at(key, (x as f32, y as f32)),
                ))
            },
        ),
    );
}
