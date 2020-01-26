#![allow(deprecated)]
//! Client error handling

error_chain! {
    foreign_links {
        Png(Box<::png::DecodingError>)
        /// Error caused a png decoding issue
        ;
        Json(::serde_json::Error)
        /// Json decoding/encoding error
        ;
        Io(::std::io::Error)
        /// Generic IO error
        ;
        Url(::url::ParseError)
        /// Url parsing errors
        ;
    }

    links {
        Common(crate::server::errors::Error, crate::server::errors::ErrorKind)
        /// Errors from the server half of the engine
        ;
    }

    errors {
        /// Errors from the lua scripting engine
        Lua(err: crate::server::lua::Error) {
            description("lua scripting error")
            display("{}", err)
        }
        /// Error returned when the target element doesn't exist
        MissingElement {
            description("missing ui element")
            display("missing ui element")
        }
        /// Error returned when resolving an address returns `None`
        AddressResolveError {
            description("failed to resolve the address")
            display("Failed to resolve the address")
        }
        /// Error returned when the ui isn't bound during scripting
        UINotBound {

        }
        /// Error returned when loading a ui node from a description file fails
        UINodeLoadError(key: crate::server::assets::ResourceKey<'static>, info: String) {
            description("Failed to parse ui node")
            display("Failed to parse node {:?}\n{}", key, info)
        }
        /// Error returned when the audio manager isn't bound during scripting
        AudioNotBound {

        }
        /// Invalid scripting state
        InvalidState {}
        /// Returned when the engine believes that a stale
        /// reference is being used.
        ///
        /// A stale reference is one that a lua script kept
        /// hold of longer than intended. This may not
        /// always be detected.
        StaleScriptReference {}
    }
}

impl From<::png::DecodingError> for Error {
    fn from(p: ::png::DecodingError) -> Error {
        ErrorKind::Png(Box::new(p)).into()
    }
}

impl From<crate::server::lua::Error> for Error {
    fn from(p: crate::server::lua::Error) -> Error {
        ErrorKind::Lua(p).into()
    }
}