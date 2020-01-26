safe_global_env.is_client = true

function init_module_scope(mod_name, scope)
    scope.ui = lock_table({
        query = function()
            return ui_root_query()
        end,
        add_node = function(node)
            return ui_add_node(node)
        end,
        remove_node = function(node)
            return ui_remove_node(node)
        end,
        load_node = function(key)
            return ui_load_node(key)
        end,
        new = function(name)
            return ui_new_node(name)
        end,
        new_text = function(value)
            return ui_new_text_node(value)
        end,
        from_str = function(str)
            return ui_new_node_str(str)
        end,
        emit_event = function(evt, ...)
            _G["ui_emit_" .. tostring(select("#", ...))](evt, ...)
        end,
        emit_accept = function(node)
            ui_emit_accept(node)
        end,
        emit_cancel = function(node)
            ui_emit_cancel(node)
        end,
        set_cursor = function(sprite)
            ui_emit_1("set_cursor", sprite)
        end,

        -- Tooltip
        show_tooltip = function(key, node, x, y)
            ui_show_tooltip(key, node, x, y)
        end,
        hide_tooltip = function(key)
            ui_hide_tooltip(key)
        end,
        move_tooltip = function(key, x, y)
            ui_move_tooltip(key, x, y)
        end,
        builder = function(inner)
            local magic_scope = setmetatable({}, {
                __metatable = false,
                __newindex = function() error("Immutable table") end,
                __index = function(tbl, name)
                    return function(values)
                        local node = scope.ui.new(name)
                        for k, v in pairs(values) do
                            if type(k) ~= "number" then
                                local ty = type(v)
                                if ty == "string" then
                                    node:set_property_string(k, v)
                                elseif ty == "number" then
                                    node:set_property_float(k, v)
                                elseif ty == "boolean" then
                                    node:set_property_bool(k, v)
                                end
                            end
                        end
                        for _, v in ipairs(values) do
                            if type(v) == "string" then
                                node:add_child(scope.ui.new_text(v))
                            else
                                node:add_child(v)
                            end
                        end
                        return node
                    end
                end,
            })
            setfenv(inner, magic_scope)
            return inner()
        end,
    })
    scope.audio = lock_table({
        play_sound = function(sound)
            audio_play_sound(mod_name, sound)
        end,
        play_sound_at = function(sound, x, y)
            return audio_play_sound_at(mod_name, sound, x, y)
        end,
    })
    scope.open_url = function(url)
        try_open_url(url)
    end
    scope.new_matrix = function()
        return new_matrix()
    end
end

function clear_module_state(mod_name)
end

function compile_ui_action(module, sub, func, elm, evt)
    local scope = get_module_scope(module)
    local sub = scope.require(sub)
    local submeta = debug.getmetatable(sub)

    if submeta.compiled_funcs == nil then
        submeta.compiled_funcs = {}
    end
    local cf = submeta.compiled_funcs[func]
    if cf ~= nil then
        return cf
    end

    local lfunc, err = loadstring("return function(node, evt)\n" .. func .. "\nend")
    if err then
        print(string.format("Failed to perform event for '%s'", module))
        error(err)
    end
    setfenv(lfunc, sub)
    local status, ret = xpcall(lfunc, gen_stack)
    if not status then
        print(string.format("Failed to perform event for '%s'", module))
        error(ret)
    end
    submeta.compiled_funcs[func] = ret
    return ret
end
