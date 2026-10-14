-- AppleHudOff.lua: hides Apple's Metal Performance HUD (the grey stats box)
-- in this app on iPhone, while the phone's own Settings > Developer >
-- Show Graphics HUD switch stays on for other apps.
--
-- Same two levers as Continuum's native/ios/AppleOverlay.swift (confirmed on
-- a real iPhone):
--   1. CAMetalLayer.developerHUDProperties = empty dictionary on every Metal
--      layer in the app's windows. Takes effect at once on live layers.
--   2. The MetalForceHudEnabled user default = NO. Metal reads it when the
--      app starts, so this half works from the next launch.
-- Runs through LuaJIT's FFI straight into the Objective-C runtime. Everything
-- is pcall'd: on any other system, or if anything is missing, it does nothing.
--
-- Personal patch, not part of the official gen1recomp release.

local M = { layersFound = 0, layersHidden = 0, ran = false }

local okFfi, ffi = pcall(require, "ffi")
if not okFfi or not (love and love.system and love.system.getOS() == "iOS") then
  return M
end

for _, decl in ipairs({
  "void *objc_getClass(const char *name);",
  "void *sel_registerName(const char *name);",
  "void objc_msgSend(void);",
  "void *objc_autoreleasePoolPush(void);",
  "void objc_autoreleasePoolPop(void *pool);",
}) do
  pcall(ffi.cdef, decl)
end

local C = ffi.C
local ok = pcall(function() return C.objc_msgSend and C.objc_getClass and C.sel_registerName end)
if not ok then return M end

local msg = C.objc_msgSend
local send = ffi.cast("void *(*)(void *, void *)", msg)
local sendP = ffi.cast("void *(*)(void *, void *, void *)", msg)
local sendIdx = ffi.cast("void *(*)(void *, void *, unsigned long)", msg)
local sendStr = ffi.cast("void *(*)(void *, void *, const char *)", msg)
local sendCount = ffi.cast("unsigned long (*)(void *, void *)", msg)
local sendBoolP = ffi.cast("bool (*)(void *, void *, void *)", msg)
local sendVoidP = ffi.cast("void (*)(void *, void *, void *)", msg)
local sendVoid = ffi.cast("void (*)(void *, void *)", msg)
local sendSetBool = ffi.cast("void (*)(void *, void *, bool, void *)", msg)

local sels = {}
local function sel(name)
  local s = sels[name]
  if not s then s = C.sel_registerName(name); sels[name] = s end
  return s
end

local function nsstring(s)
  return sendStr(C.objc_getClass("NSString"), sel("stringWithUTF8String:"), s)
end

local function isNil(p) return p == nil end

local metalLayerClass, emptyDict

-- Lever 1 on one layer and everything under it.
local function hideIn(layer, depth)
  if isNil(layer) or depth > 64 then return end
  if sendBoolP(layer, sel("isKindOfClass:"), metalLayerClass)
      and sendBoolP(layer, sel("respondsToSelector:"), sel("setDeveloperHUDProperties:")) then
    M.layersFound = M.layersFound + 1
    local props = send(layer, sel("developerHUDProperties"))
    if isNil(props) or tonumber(sendCount(props, sel("count"))) ~= 0 then
      sendVoidP(layer, sel("setDeveloperHUDProperties:"), emptyDict)
      M.layersHidden = M.layersHidden + 1
    end
  end
  local subs = send(layer, sel("sublayers"))
  if not isNil(subs) then
    for i = 0, tonumber(sendCount(subs, sel("count"))) - 1 do
      hideIn(sendIdx(subs, sel("objectAtIndex:"), i), depth + 1)
    end
  end
end

local function eachWindowArray(fn)
  local app = send(C.objc_getClass("UIApplication"), sel("sharedApplication"))
  if isNil(app) then return end
  local wins = send(app, sel("windows"))
  if not isNil(wins) then fn(wins) end
  local scenes = send(app, sel("connectedScenes"))
  if isNil(scenes) then return end
  local list = send(scenes, sel("allObjects"))
  for i = 0, tonumber(sendCount(list, sel("count"))) - 1 do
    local scene = sendIdx(list, sel("objectAtIndex:"), i)
    if sendBoolP(scene, sel("respondsToSelector:"), sel("windows")) then
      local w = send(scene, sel("windows"))
      if not isNil(w) then fn(w) end
    end
  end
end

-- Lever 1 everywhere: every Metal layer in every window of the app.
function M.apply()
  return pcall(function()
    local pool = C.objc_autoreleasePoolPush()
    local okRun, err = pcall(function()
      metalLayerClass = metalLayerClass or C.objc_getClass("CAMetalLayer")
      if isNil(metalLayerClass) then return end
      M.layersFound = 0
      if not emptyDict then
        -- kept for the whole run (alloc/init, never released)
        emptyDict = send(send(C.objc_getClass("NSDictionary"), sel("alloc")), sel("init"))
      end
      local seen = {}  -- the same window can be listed by the app and by its scene
      eachWindowArray(function(wins)
        for i = 0, tonumber(sendCount(wins, sel("count"))) - 1 do
          local win = sendIdx(wins, sel("objectAtIndex:"), i)
          local id = tonumber(ffi.cast("uintptr_t", win))
          if id ~= 0 and not seen[id] then
            seen[id] = true
            hideIn(send(win, sel("layer")), 0)
          end
        end
      end)
    end)
    C.objc_autoreleasePoolPop(pool)
    M.ran = true
    if not okRun then error(err) end
  end)
end

-- Lever 2: the launch-time request, as Continuum's syncLaunchDefault() does.
local function launchDefaultOff()
  pcall(function()
    local pool = C.objc_autoreleasePoolPush()
    pcall(function()
      local ud = send(C.objc_getClass("NSUserDefaults"), sel("standardUserDefaults"))
      sendSetBool(ud, sel("setBool:forKey:"), false, nsstring("MetalForceHudEnabled"))
      sendVoid(ud, sel("synchronize"))
    end)
    C.objc_autoreleasePoolPop(pool)
  end)
end

-- First launch after installing this copy: say what happened, once.
local function noticeOnce()
  pcall(function()
    local pool = C.objc_autoreleasePoolPush()
    pcall(function()
      local ud = send(C.objc_getClass("NSUserDefaults"), sel("standardUserDefaults"))
      local key = nsstring("g1r.appleHudOff.noticeShown.v3")
      local seen = ffi.cast("bool (*)(void *, void *, void *)", msg)(ud, sel("boolForKey:"), key)
      if seen then return end
      sendSetBool(ud, sel("setBool:forKey:"), true, key)
      sendVoid(ud, sel("synchronize"))
      if love.window and love.window.showMessageBox then
        love.window.showMessageBox("Apple stats box: OFF",
          "G1R has switched Apple's stats box off (screens found: " .. M.layersFound ..
          ").\n\nTo finish, close G1R fully (swipe it away) and open it again.",
          "info", true)
      end
    end)
    C.objc_autoreleasePoolPop(pool)
  end)
end

launchDefaultOff()
M.apply()

-- Keep it off: re-check the layers every few frames for the first ~10 s
-- (windows and layers are still being set up), then every ~5 s.
local g = love.graphics
if g and g.present then
  local present = g.present
  local frame = 0
  g.present = function(...)
    local a, b = present(...)
    frame = frame + 1
    if (frame <= 600 and frame % 15 == 0) or frame % 300 == 0 then
      M.apply()
    end
    if frame == 45 then noticeOnce() end
    return a, b
  end
end

return M
