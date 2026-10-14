-- Runs AppleHudOff.lua against the pretend runtime and checks what it did.
local ffi = require("ffi")
ffi.load(arg[1], true)  -- put the fake objc_* symbols where ffi.C looks
ffi.cdef("int report(char *buf, int n);")

local boxes = {}
local presents = 0
love = {
  system = { getOS = function() return "iOS" end },
  graphics = { present = function() presents = presents + 1 end },
  window = { showMessageBox = function(title, text) boxes[#boxes + 1] = title .. " | " .. text; return true end },
}

package.path = arg[2] .. "/?.lua;" .. package.path
local M = require("AppleHudOff")

local function state()
  local buf = ffi.new("char[256]")
  ffi.C.report(buf, 256)
  return ffi.string(buf)
end

print("after load:   ", state(), "found=" .. M.layersFound)
for _ = 1, 1200 do love.graphics.present() end
print("after 1200 frames:", state(), "found=" .. M.layersFound, "presents=" .. presents)
print("message boxes:", #boxes)
for _, b in ipairs(boxes) do print("  " .. b) end

local s = state()
assert(s:find("pool_depth=0 "), "autorelease pools unbalanced")
assert(s:find("set_calls=1 "), "should set the layer exactly once (then it stays empty)")
assert(s:find("set_with_empty=1 "), "must set an EMPTY dictionary")
assert(s:find("force_hud=0 "), "MetalForceHudEnabled must be NO")
assert(M.layersFound == 1, "should find exactly the one Metal layer")
assert(presents == 1200, "every frame must still present")
assert(#boxes == 1 and boxes[1]:find("screens found: 1"), "notice must show once, with the count")

-- Something puts a HUD request back: the next check must clear it again.
ffi.cdef("void put_back(void);")
ffi.C.put_back()
for _ = 1, 300 do love.graphics.present() end
print("after put-back + 300 frames:", state())
assert(state():find("set_calls=2 ") and state():find("set_with_empty=2 "), "must clear a HUD request that comes back")
assert(#boxes == 1, "notice must not show again")
print("ALL CHECKS PASSED")
