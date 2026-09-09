local zombies = {}
local wave, score, health = 0, 0, 100
local fire_was_down, restart_was_down = false, false

local function remove_zombie(i)
    sdk.scene.remove('zombie-body-' .. i)
    sdk.scene.remove('zombie-head-' .. i)
end

local function clear_zombies()
    for i = 1, 12 do remove_zombie(i) end
    zombies = {}
end

local function equip()
    -- These are bone-local primitives: body, barrel and grip form a tiny pistol.
    sdk.scene.held_cube('gun-body', 'RIGHTHAND', {0.00, -0.10, 0.06}, {0.10, 0.28, 0.12}, {0.12, 0.12, 0.14})
    sdk.scene.held_cube('gun-barrel', 'RIGHTHAND', {0.00, -0.24, 0.06}, {0.07, 0.30, 0.08}, {0.20, 0.20, 0.23})
    sdk.scene.held_cube('gun-grip', 'RIGHTHAND', {0.00, -0.08, -0.02}, {0.08, 0.11, 0.18}, {0.08, 0.08, 0.09})
end

local function draw_zombie(i, zombie)
    local hurt = zombie.hp < math.floor(sdk.settings.hits)
    local body_color = hurt and {0.65, 0.18, 0.12} or {0.18, 0.55, 0.16}
    sdk.scene.cube('zombie-body-' .. i, {zombie.x, zombie.y + 0.85, zombie.z}, {0.7, 1.3, 0.5}, body_color)
    sdk.scene.cube('zombie-head-' .. i, {zombie.x, zombie.y + 1.75, zombie.z}, {0.55, 0.55, 0.55}, {0.35, 0.78, 0.28})
end

local function spawn_wave()
    clear_zombies()
    wave = wave + 1
    local p = sdk.player.read().position
    local count = math.floor(sdk.settings.zombies)
    for i = 1, count do
        local angle = (i / count) * math.pi * 2 + wave * 0.37
        local radius = 12 + (i % 3) * 3
        zombies[i] = {
            x = p[1] + math.sin(angle) * radius,
            y = p[2],
            z = p[3] + math.cos(angle) * radius,
            hp = math.floor(sdk.settings.hits),
            attack = 0,
        }
        draw_zombie(i, zombies[i])
    end
end

local function hud(message)
    sdk.ui.text('zombie-hud', string.format('ZOMBIES | health %d | score %d | wave %d', health, score, wave))
    sdk.ui.text('zombie-help', message or 'RB / F: shoot | R: restart | Shots travel where the skater faces')
end

local function restart()
    score, health, wave = 0, 100, 0
    spawn_wave()
    hud()
end

local function shoot()
    if health <= 0 then return end
    local player = sdk.player.read()
    if player.bailing then return end
    local dx, dz = math.sin(player.heading), math.cos(player.heading)
    local best, best_distance
    for i, zombie in pairs(zombies) do
        local x, z = zombie.x - player.position[1], zombie.z - player.position[3]
        local forward = x * dx + z * dz
        if forward > 0 and forward < 35 then
            local side_squared = math.max(0, x * x + z * z - forward * forward)
            if side_squared < 0.75 * 0.75 and (not best_distance or forward < best_distance) then
                best, best_distance = i, forward
            end
        end
    end
    sdk.scene.cube('muzzle-flash', {
        player.position[1] + dx * 1.1,
        player.position[2] + 1.25,
        player.position[3] + dz * 1.1,
    }, {0.13, 0.13, 0.13}, {1.0, 0.65, 0.1})
    sdk.time.after('muzzle-flash', 0.06, function() sdk.scene.remove('muzzle-flash') end)
    if best then
        local zombie = zombies[best]
        zombie.hp = zombie.hp - 1
        if zombie.hp <= 0 then
            remove_zombie(best)
            zombies[best] = nil
            score = score + 100
        else
            draw_zombie(best, zombie)
        end
    end
end

local function update()
    local fire_down = sdk.input.down('KeyF') or sdk.input.action(73) > 0.5
    if fire_down and not fire_was_down then shoot() end
    fire_was_down = fire_down
    local restart_down = sdk.input.down('KeyR')
    if restart_down and not restart_was_down then restart() end
    restart_was_down = restart_down
    if health <= 0 then hud('GAME OVER | Press R to restart') else hud() end
end

local function fixed(event)
    if health <= 0 then return end
    local p = sdk.player.read().position
    local alive = 0
    for i, zombie in pairs(zombies) do
        alive = alive + 1
        local x, z = p[1] - zombie.x, p[3] - zombie.z
        local distance = math.sqrt(x * x + z * z)
        if distance > 0.001 then
            local step = math.min(distance, sdk.settings.speed * event.dt)
            zombie.x = zombie.x + x / distance * step
            zombie.z = zombie.z + z / distance * step
        end
        zombie.y = p[2]
        zombie.attack = math.max(0, zombie.attack - event.dt)
        if distance < 1.25 and zombie.attack <= 0 then
            health = math.max(0, health - 10)
            zombie.attack = 0.75
        end
        draw_zombie(i, zombie)
    end
    if alive == 0 then spawn_wave() end
    if health <= 0 then clear_zombies() end
end

return {
    on_load = function() equip(); restart(); sdk.log('Block Zombie Shooter loaded') end,
    on_update = update,
    on_fixed_update = fixed,
    on_settings = function() restart() end,
    on_event = function(event)
        if event.name == 'world_changed' then
            fire_was_down, restart_was_down = false, false
            equip()
            restart()
        end
    end,
}
