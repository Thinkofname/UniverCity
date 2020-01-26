bindgen --convert-macros --builtins --ctypes-prefix=libc --link=luajit-5.1 target/libs/include/luajit-2.0/lauxlib.h \
  --output src/lua.rs --match lua --match laux --match stddef --match stdarg

