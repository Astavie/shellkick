---- LIBRARY ----
local vec2 = vector.vec2

-- custom IR codes start from 128

-- draws a frame from fastnes
-- YIELD number (x, y, scale, instance)
local FASTNES_BG = 128

-- draws the sprites from a fastnes frame
-- YIELD number (x, y, scale, instance, xo, yo, opacity)
local FASTNES_SPR = 129

---@class Playback : Shape
---
---@field instance signal<integer>
---@field size     signal<number>
---@field offset   signal<vec2>
---@field opacity  signal<number>
---@field ghost    signal<boolean>
---
---@field width  fun(): number
---@field height fun(): number
local Playback = shapes.newshape()

---@param self Playback
---@param emit fun(...)
function Playback:draw(emit)
  if not self.ghost() then
    emit(FASTNES_BG, 0, 0, self.size(), self.instance())
  end
  emit(FASTNES_SPR, 0, 0, self.size(), self.instance(), self.offset().x, self.offset().y, self.opacity())
end

---@param pos?      signalValue<vec2,    Playback>
---@param instance? signalValue<integer, Playback>
---@param size?     signalValue<number,  Playback>
---@return Playback
---@nodiscard
function Playback.new(pos, instance, size)
  local playback = shapes.Shape(pos, { size = size or 1, offset = vec2(0), opacity = 1, ghost = false }, Playback)
  playback.instance = signal.signal(instance or 1, tweens.interp.integer, playback)

  -- divide by 3.75 to make it pixel perfect on full HD screens
  playback.width = 256 / 3.75 * playback.size
  playback.height = 240 / 3.75 * playback.size

  return playback
end

---- CODE ----
local vec2 = vector.vec2

local marios = canvas.signal("marios")
local count = #(marios())
local trait_names = {
  "patient", "bold", "twitchy", "jumpy"
}

local function personality(instance, key)
  instance = signal.as_callable(instance)
  return shapes.Text(nil, function()
    local mario = marios()[instance()]
    if key == "patient" or key == "bold" then
      return string.format("%2d", mario.personality[key])
    else
      return string.format(" %.2f", mario.personality[key])
    end
  end)
end

local function grid(n, m, width, height, f)
  local parent = shapes.Shape(vec2(-width / 2, -height / 2))
  for y = 1, m do
    for x = 1, n do
      local grid_point = shapes.Shape(vec2(width / n * (x - 0.5), height / m * (y - 0.5)))
      parent:add_child(grid_point)
      grid_point:add_child(f(x, y))
    end
  end
  return parent
end

local function focus_game(scene, root)
  local focus = Playback.new({
    x = -182 - signal.me.width / 2,
    y = -signal.me.height / 2 - 60 / 2
  }, 1, 2)

  local ghosts = shapes.Shape()
  focus:add_child(ghosts)

  local traits = grid(2, #trait_names + 1, 150, 144 / 2, function(x, y)
    if y == 1 then
      if x == 1 then
        return shapes.Text(nil, "MARIO #" .. focus.instance, 1.5)
      else
        return shapes.Shape()
      end
    end

    if x == 1 then
      return shapes.Text(nil, trait_names[y - 1])
    else
      return personality(focus.instance, trait_names[y - 1])
    end
  end)
  traits.pos(vec2(-100, -100))

  root:add_child(focus)
  root:add_child(traits)

  local two = Playback.new(vec2(100, -60/2 - 64))
  local three = Playback.new(vec2(100, -60/2))
  root:add_child(two)
  root:add_child(three)

  while true do
    -- always focus on #1
    -- refresh every half second
    scene:wait(0.1)

    local max = marios()[focus.instance()].fitness
    for i, mario in ipairs(marios()) do
      if mario.fitness > max then
        focus.instance(i)
        max = mario.fitness
      end
    end

    local next = 0
    for i, mario in ipairs(marios()) do
      if mario.fitness > next and mario.fitness < max then
        two.instance(i)
        next = mario.fitness
      end
    end

    max = next
    local next = 0
    for i, mario in ipairs(marios()) do
      if mario.fitness > next and mario.fitness < max then
        three.instance(i)
        next = mario.fitness
      end
    end

    -- refresh ghosts
    focus:remove(ghosts)
    ghosts = shapes.Shape()
    focus:add_child(ghosts)

    for i, mario in ipairs(marios()) do
      local diff = max - mario.fitness
      if diff < 200 then
        local ghost = Playback.new(nil, i, 2)
        ghost.offset({
          x = function()
            local ms = marios()
            return ms[i].fitness - ms[focus.instance()].fitness
          end,
          y = 0
        })
        ghost.opacity(0.5)
        ghost.ghost(true)
        ghosts:add_child(ghost)
      end
    end
  end
end

local function leaderboard(scene, root)
  local handle = shapes.Shape()
  root:add_child(handle)

  local positions = {}
  for i = 1, count do
    positions[i] = i
  end
  local between = signal(0)
  local positions_signal = signal(positions)
  local positions_signal_new = signal(positions)
  local start = signal(1)

  local board = shapes.Shape({
    x = -256 - (start - 1.5) * 71,
    y = 144 / 2
  })
  handle:add_child(board)

  local function pos(pos)
    return vec2((pos - 1) * 71, 0)
  end

  local handles = {}

  for i = 1, count do
    local playback = Playback.new({
      x = -signal.me.width / 2,
      y = -signal.me.height / 2
    }, i, 1)

    local position = signal(function() return positions_signal_new()[i] end)

    local traits = grid(2, #trait_names + 2, 140, 144 / 2, function(x, y)
      if y < 3 and x == 2 then
        return shapes.Shape()
      end

      if y == 1 then
        return shapes.Text(nil, "#" .. position, 1.5)
      elseif y == 2 then
        return shapes.Text(nil, "MARIO #" .. i)
      else
        if x == 1 then
          return shapes.Text(nil, trait_names[y - 2])
        else
          return personality(i, trait_names[y - 2])
        end
      end
    end)
    traits.pos({
      x = -9,
      y = 68,
    })
    traits.scale(vec2(0.3))
    playback:add_child(traits)

    playback.visible(function()
      local table = between() < 0.5 and positions_signal() or positions_signal_new()
      return table[i] >= math.floor(start()) and table[i] < math.ceil(start()) + 8
    end)

    local handle = shapes.Shape(function()
      local oldpos = positions_signal()[i]
      local newpos = positions_signal_new()[i]
      local old, new
      if between() < 0.5 then
        old = pos(oldpos)
        new = old + vec2(512 / 7, 0) * (newpos - oldpos)
      else
        new = pos(newpos)
        old = new - vec2(512 / 7, 0) * (newpos - oldpos)
      end
      return tweens.interp.linear(old, new, between())
    end)
    handle:add_child(playback)
    board:add_child(handle)

    table.insert(handles, handle)
  end

  scene:parallel(function()
    -- refresh leaderboard every second
    while true do
      local positions = {}
      local ms = marios()
      for i = 1, count do
        local me = ms[i].fitness
        local p = 1

        for j = 1, count do
          if ms[j].fitness > me or (j < i and ms[j].fitness == me) then
            p = p + 1
          end
        end

        table.insert(positions, p)
      end

      positions_signal_new(positions)
      -- between(1, 0.2)
      positions_signal(positions)
      between(0)
      scene:wait(0.3)
    end
  end)

  while true do
    start(start() + 1, 1.5)
    if start() > count then
      start(-7)
    end
  end
end

local function mario(scene, root)
  scene:parallel(focus_game, scene, root)
  scene:parallel(leaderboard, scene, root)
end

return shapes.start(mario)
