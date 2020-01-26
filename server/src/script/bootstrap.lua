-- Bootstrap code.
--
-- This provides a safe enviroment for scripts to execute in
-- which may have come from an untrusted source.
-- This uses a whitelisting system for the lua stdlib due to
-- the fact a lot of it is unsafe in some shape or form.

-- Blocks metatable access to the table and prevents mutating
-- it.
function lock_table(tbl)
    return setmetatable(tbl, {
        __metatable = false,
        __newindex = function() error("Immutable table") end,
    })
end

-- Lua's print doesn't play nice with rust's
print = function(...)
    local msg = ""
    for i = 1, select("#", ...) do
        msg = msg .. tostring(select(i, ...))
    end
    native_print(msg)
end

local control_players
function set_control_players(p)
    control_players = lock_table(p)
end

-- Init rand
math.randomseed(os.time())

-- The whilelisted enviroment of safe packages, methods to
-- call.
-- Not locked until later to allow for server/client specific
-- methods to be added
safe_global_env = {
    assert = assert,
    error = error,
    -- Metatable methods is questionable but if tables are locked then
    -- they shouldn't be an issue
    getmetatable = getmetatable,
    setmetatable = setmetatable,
    --
    ipairs = ipairs,
    pairs = pairs,
    next = next,
    pcall = pcall,
    select = select,
    tonumber = tonumber,
    tostring = tostring,
    type = type,
    unpack = unpack,
    _VERSION = _VERSION,
    xpcall = xpcall,
    -- Lua Modules
    -- Coroutine is safe
    coroutine = lock_table {
        create = coroutine.create,
        resume = coroutine.resume,
        running = coroutine.running,
        status = coroutine.status,
        wrap = coroutine.wrap,
        yield = coroutine.yield,
    },
    -- Package/module likes to load bytecode or clibs so nope
    -- Might want to provide a replacement for require though

    -- String is safe excluding dump which is a
    -- bit questionable
    string = lock_table {
        byte = string.byte,
        char = string.char,
        find = string.find,
        format = string.format,
        gmatch = string.gmatch,
        gsub = string.gsub,
        len = string.len,
        lower = string.lower,
        match = string.match,
        rep = string.rep,
        reverse = string.reverse,
        sub = string.sub,
        upper = string.upper,
        split = function(self, del)
            local ret = {};
            for match in string.gmatch(self..del, "(.-)"..del) do
                table.insert(ret, match);
            end
            return ret;
        end
    },
    -- Table is safe
    table = lock_table {
        concat = table.concat,
        insert = table.insert,
        maxn = table.maxn,
        remove = table.remove,
        sort = table.sort,
    },
    -- Math is safe
    math = lock_table {
        abs = math.abs,
        acos = math.acos,
        asin = math.asin,
        atan = math.atan,
        atan2 = math.atan2,
        ceil = math.ceil,
        cos = math.cos,
        cosh = math.cosh,
        deg = math.deg,
        exp = math.exp,
        floor = math.floor,
        fmod = math.fmod,
        frexp = math.frexp,
        huge = math.huge,
        ldexp = math.ldexp,
        log = math.log,
        log10 = math.log10,
        max = math.max,
        min = math.min,
        modf = math.modf,
        pi = math.pi,
        pow = math.pow,
        rad = math.rad,
        random = math.random,
        randomseed = math.randomseed,
        sin = math.sin,
        sinh = math.sinh,
        sqrt = math.sqrt,
        tan = math.tan,
        tanh = math.tanh,
    },
    -- No io/fs access for now
    -- May provide a limited version for savedata later

    -- No os access

    -- No debug access. That would be crazy

    -- Level read access
    level = lock_table {
        get_tile = function(x, y)
            local id = level_get_tile(x, y)
            local real  = {
                get_name = function()
                    return level_tile_name(id)
                end,
                has_property = function(prop)
                    return level_tile_prop(id, prop)
                end,
            }
            return setmetatable({}, {
                __metatable = false,
                __newindex = function() error("Immutable table") end,
                __index = real,
            })
        end,
        get_wall = function(x, y, dir)
            return level_get_wall(x, y, dir)
        end,
        get_room_type_at = function(x, y)
            return level_get_room_type_at(x, y)
        end,
        is_room_type_at = function(x, y, ty)
            return level_is_room_type_at(x, y, ty)
        end,
        get_room_display_name = function(id)
            return level_get_room_display_name(id)
        end,
    },
    get_entity_by_id = get_entity_by_id,
    game_time = function() return global_time end,
    create_global_static_entity = create_static_entity,
    -- Direction, utils
    direction = lock_table {
        ALL = lock_table({"north", "south", "east", "west"}),
        offset = function(dir)
            if dir == "north" then
                return 0, -1
            elseif dir == "south" then
                return 0, 1
            elseif dir == "east" then
                return -1, 0
            elseif dir == "west" then
                return 1, 0
            else
                error("Invalid direction " .. dir)
            end
        end,
        reverse = function(dir)
            if dir == "north" then
                return "south"
            elseif dir == "south" then
                return "north"
            elseif dir == "east" then
                return "west"
            elseif dir == "west" then
                return "east"
            else
                error("Invalid direction " .. dir)
            end
        end
    },

    -- Methods for free roaming units
    free_roam = lock_table {
        wait = function()
            coroutine.yield()
        end,
        get_entity = function()
            return coroutine.yield("entity")
        end,
        rooms_for_player = function()
            return level_get_player_rooms(coroutine.yield("player"))
        end,
        notify_player = function(de_func, data)
            coroutine.yield("notify_player")(de_func, data)
        end,
        get_idle_task = function(name)
            local player = coroutine.yield("player")
            local storage = idle_storage[player]
            if storage ~= nil then
                return storage[name]
            else
                return nil
            end
        end,
    },

    -- Methods for mission handlers to use
    control = lock_table {
        get_players = function()
            return control_players
        end,
        submit_command = function(cmd)
            control_submit_command(cmd)
        end,
        cmd = lock_table {
            exec_mission = function(data)
                return control_cmd_exec_mission(data)
            end,
        },
        get_player = function()
            return control_player
        end,
        create_static_entity = function(mdl, x, y, z)
            return create_static_entity(mdl, x, y, z)
        end,
        rooms_for_player = function(player)
            return level_get_player_rooms(player)
        end,
        give_money = function(player, amount)
            return control_give_money(player, amount)
        end,
    },
}

missions = {}
missions_by_name = {}

local function init_base_scope(mod_name, scope)
    scope.mission = lock_table {
        add = function(setup)
            local name = setup.name or error("missing mission name")
            local handler = setup.handler or error("missing mission handler")
            local description = setup.description or error("missing mission description")
            local save_key = setup.save_key or error("missing mission save_key")
            local name_key = name
            if not string.find(name_key, ":") then
                name_key = mod_name .. ":" .. name_key
            end
            local m = {
                mod = mod_name,
                name = name,
                handler = handler,
                description = description,
                save_key = save_key,
            }
            table.insert(missions, m)
            missions_by_name[name_key] = m
            print("Registered mission " .. m.name .. " with handler " .. m.handler)
        end,
    }
end

-- Patch up strings
local string_meta = getmetatable("")
string_meta.__index = safe_global_env.string
string_meta.__metatable = false

local module_scopes = {}
local modules = {}

-- Returns a module specific scope to be used when invoking module
-- methods. Cache's scopes and only creates a scope if one with the
-- given mod_name doesn't exist.
function get_module_scope(mod_name)
    local scope = module_scopes[mod_name]
    if scope == nil then
        scope = {
            -- Make it easier to work out which module sent
            -- the print
            print = function(...)
                local msg = ""
                for i = 1, select("#", ...) do
                    msg = msg .. tostring(select(i, ...))
                end
                native_print_mod(msg, mod_name)
            end,
            _LOADED = lock_table({}),
        }
        -- This somewhat matches lua's builtin `require` but with
        -- some differences. For example instead of having to
        -- return a module table scripts loaded by this have
        -- their own global scope which inherits from the module
        -- scope making each script independent from each other.
        scope.require = function(lib)
            local mo = string.find(lib, ":")
            if mo then
                local scope = get_module_scope(string.sub(lib, 1, mo - 1))
                return scope.require(string.sub(lib, mo + 1))
            end
            local loaded = scope._LOADED
            -- Bad attempt to prevent scripts leaving the `./scripts/`
            -- folder. The asset loader (used by get_module_script) will
            -- prevent it leaving the module's base folder however so
            -- this isn't a major issue.
            local lib = lib:gsub("/", ""):gsub("%.%.", "")
            local mod = loaded[lib]
            if mod == nil then
                -- Lua apparently uses `.` as a seperator
                local lib_file = lib:gsub("%.", "/")
                local file = get_module_script(mod_name, "scripts/" .. lib_file .. ".lua")
                local script, err = loadstring(file, mod_name .. ":" .. lib_file .. ".lua")
                if script == nil then
                    error(err)
                end
                local scope = get_module_scope(mod_name)
                -- Give the lib its own scope but have it share the global module scope
                local inner_scope = setmetatable({}, {
                    __metatable = false,
                    __index = scope,
                })
                setfenv(script, inner_scope)
                rawset(loaded, lib, inner_scope)
                mod = inner_scope
                script()
            end
            return mod
        end
        init_base_scope(mod_name, scope)
        init_module_scope(mod_name, scope)
        setmetatable(scope, {
            __metatable = false,
            __newindex = function() error("Immutable table") end,
            __index = safe_global_env,
        })
        module_scopes[mod_name] = scope
    end
    return scope
end

-- Forces the named script to be reloaded
function reload_module(module, file)
    clear_module_state(module)
    local scope = get_module_scope(module)
    local old_loaded = scope._LOADED
    scope._LOADED = {}
    for k, v in pairs(old_loaded) do
        if rawget(v, "NO_RELOAD") then
            print(tostring(k) .. " can't be reloaded for " .. module)
            rawset(scope._LOADED, k, v)
        end
    end

    for k, v in pairs(old_loaded) do
        if not rawget(v, "NO_RELOAD") then
            local status, err = pcall(function() scope.require(k) end)
            if not status then
                print("Reload failed for " .. tostring(k))
                print(err)
                rawset(scope._LOADED, k, v)
            else
                print("Reloaded " .. tostring(k) .. " for " .. module)
            end
        end
    end
end

-- Invokes the named method from the module
-- loading the sub-script if required
function invoke_module_method(module, sub, method, ...)
    local scope = get_module_scope(module)
    local sub = scope.require(sub)
    return sub[method](...)
end

-- Invokes the named method from the module
-- as a coroutine.
function invoke_free_roam(module, sub, method, existing, c_scope)
    local scope = get_module_scope(module)
    local sub = scope.require(sub)
    if existing == nil or coroutine.status(existing) == "dead" then
        existing = coroutine.create(function()
            sub[method]()
        end)
        return existing
    end

    local param = nil
    while true do
        local ok, val = coroutine.resume(existing, param)
        param = nil
        if not ok then
            error(val)
        end
        if c_scope[val] ~= nil then
            param = c_scope[val]
        else
            break
        end
    end

    return existing
end

-- Appends a stack trace to the passed error.
--
-- Useful for xpcall to retain the stacktrace on error
function gen_stack(err)
    return err .. "\n" .. debug.traceback()
end

-- Loads the named module by invoking its init script
--
-- Returns whether the module loaded successfully or not
function load_module(mod_name)
    local status, init_file = xpcall(get_module_script, gen_stack, mod_name, "scripts/init.lua")
    if not status then
        error(init_file)
    end
    local init_script = loadstring(init_file, mod_name .. ":init.lua")
    local mod_scope = get_module_scope(mod_name)
    local inner_scope = setmetatable({}, {
        __metatable = false,
        __index = mod_scope,
    })
    modules[mod_name] = inner_scope
    rawset(mod_scope._LOADED, "init", inner_scope) -- require 'init' == module
    setfenv(init_script, inner_scope)
    local status, err = xpcall(init_script, gen_stack)
    if status then
        print("Loaded module: " .. mod_name)
        return true
    else
        print("Error loading module: " .. mod_name)
        print("Error was: " .. err)
        return false
    end
end

-- Called by the game after all boot scripts are loaded
function setup()
    lock_table(safe_global_env)
end

-- Serialization lib

safe_global_env.serialize = lock_table({
    define = function(desc)
        local native_desc = serialize_create_desc(desc)
        return {
            decode = function(data)
                return serialize_decode(native_desc, data)
            end,
            encode = function(data)
                return serialize_encode(native_desc, data)
            end,
        }
    end,
})