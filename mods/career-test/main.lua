-- Dev only (branch world/challenges, doc 43). Walks the career once: skip the start chain, add 15,000 board sales
-- (milestone 1), then report the milestone facts. Every step logs a CAREER_TEST line for automated runs.
local step = 0
local wait = 0

local function log(text) sdk.log("CAREER_TEST " .. text) end

local function milestone()
  local m = sdk.career.get().milestone or {}
  return string.format("index=%s count=%s start=%s goal=%s pending=%s sales=%s", tostring(m.index), tostring(m.count),
    tostring(m.start), tostring(m.goal), tostring(m.pending), tostring(sdk.career.get().board_sales))
end

return {
  on_update = function(event)
    if not sdk.career.on() then return end
    wait = wait - ((event and tonumber(event.dt)) or 0)
    if wait > 0 then return end
    wait = 1
    if step == 0 then
      log("start " .. milestone())
      sdk.career.skip_start()
      sdk.career.set_flag("ran", true)
    elseif step == 1 then
      log("after skip_start " .. milestone())
      -- Added by this mod's progression/test.json overlay; fires on the sales below.
      sdk.career.arm_action("career_test_bonus")
      sdk.career.add_sales(15000, "career-test")
    elseif step == 2 then
      log("after add_sales " .. milestone())
      log("done")
    end
    step = step + 1
  end,
  on_event = function(event)
    if type(event) == "table" and type(event.name) == "string" and event.name:sub(1, 7) == "career_" then
      log("event " .. event.name .. " " .. tostring(event.action or event.achievement or event.tier or event.unlock or event.challenge or ""))
    end
  end,
}
