//! Common struct definitions shared between multiple modules

mod money;
pub use self::money::*;
use crate::prelude::*;

use delta_encode::{bitio, DeltaEncodable};
use std::sync::Arc;
use std::io;
use crate::assets;

/// A collection of animation descriptions
pub type AnimationSet = Arc<FNVMap<String, Animation>>;

/// A description of a set of animations to play on a model
pub struct Animation {
    /// Whether the animation should loop once complete or just
    /// stay on the last animation.
    pub should_loop: bool,
    /// List of animations to play
    pub animations: Vec<assets::ResourceKey<'static>>
}


/// JSON'able version of `Animation`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnimationInfo {
    #[serde(rename="loop")]
    should_loop: bool,
    animations: Vec<String>,
}

impl AnimationInfo {
    /// Converts the JSON'able collection of animations into a `AnimationSet`
    pub fn map_to_animation_set(module: assets::ModuleKey<'_>, vals: FNVMap<String, AnimationInfo>) -> AnimationSet {
        Arc::new(vals.into_iter()
            .map(|(k, v)| (k, Animation {
                should_loop: v.should_loop,
                animations: v.animations.into_iter()
                    .map(|v| assets::LazyResourceKey::parse(&v)
                        .or_module(module.borrow())
                        .into_owned()
                    )
                    .collect()
            }))
            .collect())
    }
}

/// Describes an entry into the mission table
#[derive(Debug, Deserialize, Clone)]
pub struct MissionEntry {
    /// The mod that created this mission
    #[serde(rename = "mod")]
    pub mod_name: String,
    /// The name of this mission
    pub name: String,
    /// The handler of this mission
    handler: String,
    /// The description of this mission
    description: String,
    /// The key to use when saving this mission
    pub save_key: String,
}

impl MissionEntry {
    /// Returns the resource key pointing used to reference
    /// this mission
    pub fn get_name_key(&self) -> ResourceKey<'_> {
        ResourceKey::new(self.mod_name.as_str(), self.name.as_str())
    }

    /// Returns the resource key pointing to the lua module
    /// that will handle this mission
    pub fn get_handler_key(&self) -> ResourceKey<'_> {
        LazyResourceKey::parse(&self.handler)
            .or_module(ModuleKey::new(self.mod_name.as_str()))
    }

    /// The description of this mission with formatting fixed
    pub fn get_description(&self) -> String {
        let desc = self.description.trim();
        let mut out = String::with_capacity(desc.len());
        for line in desc.lines() {
            let line = line.trim();
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str(line);
                out.push(' ');
            }
        }
        out
    }
}

/// Serializes script generated data
#[derive(Clone, Debug)]
pub struct ScriptData(pub Arc<bitio::Writer<Vec<u8>>>);

impl PartialEq for ScriptData {
    fn eq(&self, other: &ScriptData) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || self.0 == other.0
    }
}

impl DeltaEncodable for ScriptData {

    fn encode<W>(&self, base: Option<&Self>, w: &mut bitio::Writer<W>) -> io::Result<()>
        where W: io::Write
    {
        if base.map_or(false, |v| v == self) {
            w.write_bool(false)?;
        } else {
            w.write_bool(true)?;
            bitio::write_len_bits(w, self.0.bit_len())?;
            self.0.copy_into(w)?;
        }
        Ok(())
    }

    fn decode<R>(base: Option<&Self>, r: &mut bitio::Reader<R>) -> io::Result<Self>
        where R: io::Read
    {
        use std::cmp;
        if r.read_bool()? {
            let mut num_bits = bitio::read_len_bits(r)?;
            let mut buf = bitio::Writer::new(vec![]);
            while num_bits > 0 {
                let bits = cmp::min(32, num_bits);
                buf.write_unsigned(r.read_unsigned(bits as u8)?, bits as u8)?;
                num_bits -= bits;
            }
            Ok(ScriptData(Arc::new(buf)))
        } else if let Some(base) = base {
            Ok(base.clone())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "Missing previous state"))
        }
    }
}