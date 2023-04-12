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
  "patient", "random", "playful", "twitchy", "smart"
}

local function personality(instance, key)
  return shapes.Text(nil, function()
    local mario = marios()[instance()]
    return mario.personality[key] .. ""
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
    y = -signal.me.height / 2 - 144 / 2
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

  while true do
    -- always focus on #1
    -- refresh every half second
    scene:wait(0.5)

    local max = marios()[focus.instance()].fitness

    for i, mario in ipairs(marios()) do
      if mario.fitness > max then
        focus.instance(i)
        max = mario.fitness
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

local function swap(parent, a, b, dir, time, easing)
  local offset = signal(vec2(0))
  local swap_a = shapes.Shape(offset)
  local swap_b = shapes.Shape(offset - dir)

  swap_a:add_child(a)
  swap_b:add_child(b)

  parent:add_child(swap_a)
  parent:add_child(swap_b)

  offset(dir, time, easing)

  parent:add_child(b)
  parent:remove(swap_a)
  parent:remove(swap_b)
  return b
end

local function leaderboard(scene, root)
  local handle = shapes.Shape({
    x = 0,
    y = 144 / 2,
  })
  root:add_child(handle)

  local board = shapes.Shape()
  handle:add_child(board)

  local indices = {}
  for i = 1, count do
    indices[i] = i
  end

  scene:parallel(function()
    -- refresh leaderboard every second
    local function swap(a, b)
      local ia = indices[a]
      local ib = indices[b]

      if ia == nil or ib == nil then
        return false
      end

      local sa = marios()[ia].fitness
      local sb = marios()[ib].fitness

      if sa < sb then
        indices[a], indices[b] = ib, ia
        return true
      end

      return false
    end

    while true do
      scene:wait(1)
      for i = 1, count do
        local ci = i
        ::redo::
        if swap(ci, ci+1) then
          ci = ci - 1
          goto redo
        end
      end
    end
  end)

  local start = 1

  while true do
    board = swap(handle, board, grid(7, 2, 512, 137, function(x, y)
      local position = start + (x - 1) + (y - 1) * 7
      if position > count then
        return shapes.Shape()
      else
        local index = indices[position]
        local playback = Playback.new({
          x = -signal.me.width / 2,
          y = -signal.me.height / 2
        }, index, 1)

        local traits = grid(1, #trait_names + 1, 150, 144 / 2, function(_, y)
          if y == 1 then
            return shapes.Text(nil, "#" .. index, 1.5)
          else
            return personality(function() return index end, trait_names[y - 1])
          end
        end)
        traits.pos({
          x = 35,
          y = 10,
        })
        traits.scale(vec2(0.3))
        playback:add_child(traits)

        return playback
      end
    end), vec2(-512, 0), 1)

    scene:wait(5)

    start = start + 14
    if start > count then
      start = 1
    end
  end
end

local function mario(scene, root)
  scene:parallel(focus_game, scene, root)
  scene:parallel(leaderboard, scene, root)
end

return shapes.start(mario)
