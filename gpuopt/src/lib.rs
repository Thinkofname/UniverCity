

/// Puts flags into the binary that tries to force the driver
/// to use the high power gpu vs the integrated one on devices
/// with two gpus
#[macro_export]
macro_rules! try_force_gpu {
    () => {
        #[no_mangle]
        #[allow(non_upper_case_globals)]
        #[allow(missing_docs)]
        #[used]
        pub static NvOptimusEnablement: u32 = 1;

        #[no_mangle]
        #[allow(non_upper_case_globals)]
        #[allow(missing_docs)]
        #[used]
        pub static AmdPowerXpressRequestHighPerformance: u32 = 1;
    }
}
