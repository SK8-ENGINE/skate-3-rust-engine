local function draw()
    if not sdk.settings.visible then sdk.scene.remove('speed'); return end
    local v=sdk.player.read().velocity
    local speed=math.sqrt(v[1]^2+v[2]^2+v[3]^2)
    local units=sdk.settings.units
    speed=speed*(units=='km/h' and 3.6 or units=='mph' and 2.236936 or 1)
    sdk.ui.text('speed',string.format('%s: %.1f %s',sdk.settings.label,speed,units))
end
return {
    on_load=function() sdk.log('Speedometer loaded; mutable state starts fresh on reload.'); draw() end,
    on_update=draw,
    on_settings=draw,
    on_event=function(e) if e.name=='world_changed' then draw() end end,
    on_unload=function() -- Owned text is removed by the host, even if this callback fails.
    end
}
