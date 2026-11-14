-- Dev only (doc 26h, Traffic for mods): every census category spawns the mod's own taxi type.
local done = false
local categories = { "hatchbacks", "minivans", "muscles", "pickups", "sedans", "sports", "suvs", "taxis" }

return {
  on_update = function()
    if done then return end
    done = true
    local tables = {}
    for _, c in ipairs(categories) do tables[c] = { "my_cab" } end
    sdk.world.set_tuning("vehicles", { entities = { my_cab = "taxi01" }, spawn_tables = tables })
    sdk.world.tuning("vehicles_tables", "vehicles")
    sdk.log("VEHICLES_TEST tables set")
  end,
  on_event = function(event)
    if event and event.key == "vehicles_tables" then
      sdk.log("VEHICLES_TEST inspect " .. tostring(event.result and event.result.spawn_tables and event.result.spawn_tables.taxis and event.result.spawn_tables.taxis[1]))
    end
  end,
}
