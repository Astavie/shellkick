---- LIBRARY ----

-- custom IR codes start from 128

-- draws a frame from fastnes
-- YIELD number (x, y, scale, instance)
ir.FASTNES = 128

---@class Playback : Shape
---@field instance signal<integer>
---@field size signal<number>
local Playback = shapes.newshape()

---@param self Playback
---@param emit fun(...)
function Playback:draw(emit)
  emit(ir.FASTNES, 0, 0, self.size(), self.instance())
end

---@param pos?      signalValue<vec2,    Playback>
---@param instance? signalValue<integer, Playback>
---@param size?     signalValue<number,  Playback>
---@return Playback
---@nodiscard
function Playback.new(pos, instance, size)
  local playback = shapes.Shape(pos, { size = size or 1 }, Playback)
  playback.instance = signal.signal(instance or 1, tweens.interp.integer, playback)

  -- divide by 3.75 to make it pixel perfect on full HD screens
  playback.width = 256 / 3.75 * playback.size
  playback.height = 240 / 3.75 * playback.size

  return playback
end

---- CODE ----
local vec2 = vector.vec2

local function mario(scene, root)
  local playback = shapes.Shape():add_child(Playback.new({
    x = -signal.me.width / 2,
    y = -signal.me.height / 2
  }))

  root:add_child(playback)

  local text = shapes.Text({
    x = -signal.me.width / 2,
    y = -100,
  }, "frame " .. canvas.signal("frame"))

  root:add_child(text)

  scene:advance(playback.angle, math.pi / 2)
end

return shapes.start(mario)
