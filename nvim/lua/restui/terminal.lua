local M = {}
local config = require("restui.config")

local buf, win

function M.toggle()
    if win and vim.api.nvim_win_is_valid(win) then
        vim.api.nvim_win_close(win, true)
        win = nil
        return
    end

    local opts = config.options.float_opts
    local width = math.floor(vim.o.columns * opts.width)
    local height = math.floor(vim.o.lines * opts.height)
    local row = math.floor((vim.o.lines - height) / 2)
    local col = math.floor((vim.o.columns - width) / 2)

    buf = vim.api.nvim_create_buf(false, true)
    win = vim.api.nvim_open_win(buf, true, {
        relative = "editor",
        width = width,
        height = height,
        row = row,
        col = col,
        border = opts.border,
        style = "minimal",
    })

    -- Build command with context
    local cmd = config.options.restui_cmd
    local file = vim.fn.expand("%:p")
    if file:match("%.http$") or file:match("%.yaml$") or file:match("%.yml$") then
        cmd = cmd .. " --file " .. vim.fn.shellescape(file)
    end

    if config.options.env_file then
        cmd = cmd .. " --env-file " .. vim.fn.shellescape(config.options.env_file)
    end

    -- Pass current working directory
    local cwd = vim.fn.getcwd()
    cmd = cmd .. " --dir " .. vim.fn.shellescape(cwd)

    vim.fn.termopen(cmd, {
        on_exit = function()
            if win and vim.api.nvim_win_is_valid(win) then
                vim.api.nvim_win_close(win, true)
                win = nil
            end
        end,
    })

    vim.cmd("startinsert")
end

return M
