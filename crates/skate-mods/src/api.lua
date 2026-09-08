local submit = sdk._submit
sdk._submit = nil
function sdk.log(text) submit{kind="log",text=text} end
sdk.ui = {}
function sdk.ui.text(key,text) submit{kind="overlay",key=key,text=text} end
sdk.scene = {}
function sdk.scene.cube(key,position,size,color)
    submit{kind="cube",key=key,position=position,size=size,color=color}
end
function sdk.scene.remove(key) submit{kind="remove",key=key} end
sdk.player = {}
function sdk.player.read() return sdk.snapshot.player end
function sdk.player.teleport(position,heading,on_board)
    submit{kind="teleport",position=position,heading=heading,on_board=on_board}
end
sdk.input = {}
function sdk.input.down(key) return sdk.snapshot.keys[key] == true end
-- Timers use active Update time. Reusing a key replaces the old timer.
sdk.time = { elapsed = 0 }
local timers = {}
function sdk.time.after(key,seconds,callback)
    assert(type(key)=='string' and #key>0 and #key<=64,'invalid timer key')
    assert(type(seconds)=='number' and seconds>=0 and seconds<=86400,'invalid timer delay')
    assert(type(callback)=='function','timer callback must be a function')
    local count=0; for _ in pairs(timers) do count=count+1 end
    assert(timers[key] or count<64,'64 timers maximum')
    timers[key]={at=sdk.time.elapsed+seconds,callback=callback}
end
function sdk.time.cancel(key) timers[key]=nil end
function sdk._advance(dt)
    sdk.time.elapsed=sdk.time.elapsed+dt
    local due={}
    for key,timer in pairs(timers) do if timer.at<=sdk.time.elapsed then due[#due+1]=key end end
    table.sort(due)
    for _,key in ipairs(due) do
        local timer=timers[key]
        if timer and timer.at<=sdk.time.elapsed then timers[key]=nil; timer.callback() end
    end
end
function sdk.input.action(id)
    assert(type(id)=='number' and id%1==0 and id>=64 and id<=81,'action ID must be 64..81')
    return sdk.snapshot.actions[id-63]
end

sdk.animation = {}
function sdk.animation.replace(path) submit{kind="animation",path=path} end
function sdk.animation.info() return sdk.snapshot.animation end
