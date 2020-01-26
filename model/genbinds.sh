bindgen --convert-macros --builtins --ctypes-prefix=libc --link=assimp assimp.h --output src/assimp.rs --match stddef --match stdarg --match assimp --no-rust-enums
sed -i '1,6d' src/assimp.rs