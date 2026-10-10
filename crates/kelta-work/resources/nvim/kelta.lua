-- Kelta review notes (#133). Kelta writes this file to <data>/nvim/kelta.lua and loads it with
-- `--cmd 'lua dofile(...)'` only in the nvim it spawns; it talks to Kelta's ctl socket ($KELTA_SOCK)
-- one request at a time, nothing stays resident.
if vim.g.loaded_kelta_notes or not vim.env.KELTA_SOCK or not vim.env.KELTA_SESSION_ID then
  return
end
vim.g.loaded_kelta_notes = true

local ns = vim.api.nvim_create_namespace('kelta_notes')
local state = { worktree = nil, notes = {} }

-- One ctl request: `{"v":1,"cmd":...,"session":...}` → result, or nil + error message.
local function ctl(req)
  req.v = 1
  req.session = vim.env.KELTA_SESSION_ID
  local out, done = '', false
  local ok, ch = pcall(vim.fn.sockconnect, 'pipe', vim.env.KELTA_SOCK, {
    on_data = function(_, data)
      out = out .. table.concat(data, '\n')
      done = done or out:find('\n') ~= nil or (#data == 1 and data[1] == '')
    end,
  })
  if not ok or ch == 0 then
    return nil, 'Kelta is not reachable'
  end
  vim.fn.chansend(ch, vim.json.encode(req) .. '\n')
  vim.wait(10000, function()
    return done
  end, 10)
  pcall(vim.fn.chanclose, ch)
  local line = out:match('^[^\n]+')
  if not line then
    return nil, 'Kelta did not answer'
  end
  local resp = vim.json.decode(line)
  if not resp.ok then
    return nil, (type(resp.error) == 'table' and resp.error.message) or 'request refused'
  end
  return resp.result
end

local function abs(path)
  if path:sub(1, 1) == '/' or not state.worktree then
    return path
  end
  return state.worktree .. '/' .. path
end

local function hl(note)
  if note.state == 'untouched' then
    return 'DiagnosticWarn'
  elseif note.state == 'touched' then
    return 'DiagnosticOk'
  end
  return 'DiagnosticInfo'
end

local function place(buf)
  if not vim.api.nvim_buf_is_loaded(buf) then
    return
  end
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  local name = vim.api.nvim_buf_get_name(buf)
  if name == '' then
    return
  end
  local last = vim.api.nvim_buf_line_count(buf)
  for _, n in ipairs(state.notes) do
    if n.state ~= 'resolved' and abs(n.path) == name then
      pcall(vim.api.nvim_buf_set_extmark, buf, ns, math.min(n.line_start, last) - 1, 0, {
        sign_text = 'K>',
        sign_hl_group = hl(n),
        virt_text = { { 'K> ' .. n.body, hl(n) } },
        virt_text_pos = 'eol',
      })
    end
  end
end

-- `ReviewNotes` from Kelta (a ctl answer, or pushed over RPC when notes change elsewhere).
local function apply(view)
  if type(view) ~= 'table' then
    return
  end
  state.worktree = view.worktree
  state.notes = view.notes or {}
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    place(b)
  end
end
_G.KeltaNotes = { apply = apply }

local function call(req)
  local res, err = ctl(req)
  if err then
    vim.notify('Kelta: ' .. err, vim.log.levels.ERROR)
    return nil
  end
  apply(res)
  return res
end

local function add(l1, l2)
  local path = vim.api.nvim_buf_get_name(0)
  if path == '' then
    vim.notify('Kelta: this buffer has no file', vim.log.levels.WARN)
    return
  end
  local where = l1 == l2 and ('L' .. l1) or ('L' .. l1 .. '-' .. l2)
  vim.ui.input({ prompt = 'Note ' .. where .. ': ' }, function(body)
    if body and vim.trim(body) ~= '' then
      call({ cmd = 'note_add', path = path, line_start = l1, line_end = l2, body = body })
    end
  end)
end

local function list()
  if not call({ cmd = 'note_list' }) then
    return
  end
  local items = {}
  for _, n in ipairs(state.notes) do
    if n.state ~= 'resolved' then
      items[#items + 1] = {
        filename = abs(n.path),
        lnum = n.line_start,
        end_lnum = n.line_end,
        text = '[' .. n.state .. '] ' .. n.body,
      }
    end
  end
  vim.fn.setqflist({}, ' ', { title = 'Kelta review notes', items = items })
  vim.cmd('copen')
end

local function send()
  local res = call({ cmd = 'note_send' })
  if res then
    vim.notify('Kelta: review notes sent to Claude')
  end
end

-- Keymaps once the user's config has set <leader>; notes re-placed from Kelta on attach.
vim.api.nvim_create_autocmd('VimEnter', {
  once = true,
  callback = function()
    vim.keymap.set('n', '<leader>kn', function()
      local l = vim.fn.line('.')
      add(l, l)
    end, { desc = 'Kelta: review note on this line' })
    vim.keymap.set('x', '<leader>kn', function()
      local a, b = vim.fn.line('v'), vim.fn.line('.')
      vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<Esc>', true, false, true), 'nx', false)
      add(math.min(a, b), math.max(a, b))
    end, { desc = 'Kelta: review note on these lines' })
    vim.keymap.set('n', '<leader>kN', list, { desc = 'Kelta: review notes in quickfix' })
    vim.keymap.set('n', '<leader>ks', send, { desc = 'Kelta: send review notes to Claude' })
    vim.api.nvim_create_autocmd('BufWinEnter', {
      group = vim.api.nvim_create_augroup('kelta_notes', { clear = true }),
      callback = function(ev)
        place(ev.buf)
      end,
    })
    local res = ctl({ cmd = 'note_list' })
    apply(res)
  end,
})
