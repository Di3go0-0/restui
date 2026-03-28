local M = {}

M.defaults = {
    restui_cmd = "restui",
    float_opts = {
        width = 0.9,
        height = 0.9,
        border = "rounded",
    },
    open_on_http_file = false,
    env_file = nil,
    keymap = "<leader>rr",
}

M.options = {}

function M.setup(opts)
    M.options = vim.tbl_deep_extend("force", {}, M.defaults, opts or {})
end

return M
