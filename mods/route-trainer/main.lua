local points, previous, next_point, elapsed, running = {}, {}, 1, 0, false
local function pressed(key)
    local down=sdk.input.down(key); local edge=down and not previous[key]; previous[key]=down; return edge
end
local function clear()
    sdk.time.cancel('result')
    for i=1,#points do sdk.scene.remove('gate'..i) end
    points={}; next_point=1; elapsed=0; running=false
end
local function status(text) sdk.ui.text('route',text) end
local function update(e)
    local p=sdk.player.read().position
    if pressed(sdk.settings.record_key) and #points<32 and not running then
        points[#points+1]={p[1],p[2],p[3]}
        sdk.scene.cube('gate'..#points,{p[1],p[2]+0.5,p[3]},{0.2,1,0.2},{0.1,0.8,0.6})
    end
    if pressed('F8') then clear() end
    if pressed('F7') and #points>1 then sdk.time.cancel('result'); next_point=1; elapsed=0; running=true end
    if running then
        elapsed=elapsed+e.dt
        local q=points[next_point]
        local distance=math.sqrt((p[1]-q[1])^2+(p[2]-q[2])^2+(p[3]-q[3])^2)
        if distance<=sdk.settings.radius then
            next_point=next_point+1
            if next_point>#points then
                running=false
                status(string.format('Finished! %.2f s',elapsed))
                sdk.time.after('result',5,function() status('F7: run again | F8: clear route') end)
                return
            end
        end
        status(string.format('Checkpoint %d/%d | %.2f s',next_point,#points,elapsed))
    else -- Retain finished result until the one-shot timer replaces it.
        if elapsed==0 then status(string.format('%d checkpoints | %s add | F7 start | F8 clear',#points,sdk.settings.record_key)) end
    end
end
return {
    on_load=function() status('Record a route with '..sdk.settings.record_key) end,
    on_update=update,
    on_event=function(e)
        if e.name=='world_changed' then clear(); sdk.time.cancel('result'); status('New world: route cleared') end
        if e.name=='bail_changed' and e.bailing and sdk.settings.reset_on_bail then running=false; status('Bailed. F7 restarts the run.') end
    end,
    on_settings=function() previous={} end
}
