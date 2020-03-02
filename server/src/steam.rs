//! Helper for working with steam

#[cfg(feature = "steam")]
use crate::saving::filesystem::*;
#[cfg(feature = "steam")]
use steamworks;

/// A portable subset of the steam api that both clients
/// and servers can use.
///
/// Used to share the client's steam client to avoid
/// duplication.
#[allow(missing_docs)]
pub trait Steam {
    #[cfg(feature = "steam")]
    fn steam_id(&self) -> steamworks::SteamId;
    #[cfg(feature = "steam")]
    fn begin_authentication_session(
        &self,
        user: steamworks::SteamId,
        ticket: &[u8],
    ) -> Result<(), steamworks::AuthSessionError>;
    #[cfg(feature = "steam")]
    fn end_authentication_session(&self, user: steamworks::SteamId);
}

#[cfg(not(feature = "steam"))]
impl Steam for () {}

#[cfg(feature = "steam")]
impl<Manager> Steam for steamworks::Client<Manager> {
    fn steam_id(&self) -> steamworks::SteamId {
        self.user().steam_id()
    }
    fn begin_authentication_session(
        &self,
        user: steamworks::SteamId,
        ticket: &[u8],
    ) -> Result<(), steamworks::AuthSessionError> {
        self.user().begin_authentication_session(user, ticket)
    }
    fn end_authentication_session(&self, user: steamworks::SteamId) {
        self.user().end_authentication_session(user)
    }
}

#[cfg(feature = "steam")]
impl Steam for steamworks::Server {
    fn steam_id(&self) -> steamworks::SteamId {
        steamworks::Server::steam_id(self)
    }
    fn begin_authentication_session(
        &self,
        user: steamworks::SteamId,
        ticket: &[u8],
    ) -> Result<(), steamworks::AuthSessionError> {
        steamworks::Server::begin_authentication_session(self, user, ticket)
    }
    fn end_authentication_session(&self, user: steamworks::SteamId) {
        steamworks::Server::end_authentication_session(self, user)
    }
}

/// A filesystem that uses the steam cloud to store files
#[derive(Clone)]
#[cfg(feature = "steam")]
pub struct SteamCloud<Manager> {
    rs: steamworks::RemoteStorage<Manager>,
}

#[cfg(feature = "steam")]
impl<Manager> SteamCloud<Manager> {
    /// Uses the passed storage to access the steam cloud as a filesystem
    pub fn new(rs: steamworks::RemoteStorage<Manager>) -> SteamCloud<Manager> {
        SteamCloud { rs }
    }
}

#[cfg(feature = "steam")]
impl<Manager> FileSystem for SteamCloud<Manager> {
    type Reader = steamworks::SteamFileReader<Manager>;
    type Writer = steamworks::SteamFileWriter<Manager>;

    fn read(&self, name: &str) -> std::io::Result<Self::Reader> {
        let f = self.rs.file(name);
        if !f.exists() {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        Ok(f.read())
    }

    fn write(&self, name: &str) -> std::io::Result<Self::Writer> {
        Ok(self.rs.file(name).write())
    }

    fn delete(&self, name: &str) -> std::io::Result<()> {
        self.rs.file(name).delete();
        Ok(())
    }

    fn exists(&self, name: &str) -> bool {
        self.rs.file(name).exists()
    }

    fn timestamp(&self, name: &str) -> std::io::Result<chrono::DateTime<chrono::Local>> {
        let f = self.rs.file(name);
        if !f.exists() {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        use chrono::TimeZone;
        let ts = f.timestamp();
        Ok(chrono::Local.timestamp(ts, 0))
    }

    fn files(&self) -> Vec<String> {
        self.rs.files().into_iter().map(|v| v.name).collect()
    }
}
