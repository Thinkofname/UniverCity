
struct RustClosureData {
    int (*cfunc)(void*);
};

// Save having to include the header
extern void* lua_touserdata(void*, int);
extern void lua_error(void*);

int invoke_rust_closure(void* state) {
    struct RustClosureData* func = lua_touserdata(state, -10003);
    int ret = (func->cfunc)(state);
    if (ret == -1) {
        lua_error(state);
    }
    return ret;
}