local function apply()
    sdk.trainer.apply(sdk.settings)
end
return {
    on_load = apply,
    on_settings = apply,
    on_event = function(event)
        if event.name == 'world_changed' then apply() end
    end
}
