local M = {}

function M.setup(opts)
    require("restui.config").setup(opts)
    local config = require("restui.config").options

    vim.api.nvim_create_user_command("Restui", function()
        require("restui.terminal").toggle()
    end, { desc = "Toggle restui HTTP client" })

    if config.keymap then
        vim.keymap.set("n", config.keymap, function()
            require("restui.terminal").toggle()
        end, { desc = "Toggle restui HTTP client" })
    end

    -- Auto-open on .http files (optional)
    if config.open_on_http_file then
        vim.api.nvim_create_autocmd("BufEnter", {
            pattern = { "*.http", "*.rest" },
            callback = function()
                require("restui.terminal").toggle()
            end,
        })
    end
end

return M
