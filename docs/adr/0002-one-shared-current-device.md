# One shared current device

herdr-web-browser gives each agent pane its own tab, but here every agent and the pane act on one shared current device, chosen by `boot`, and an agent picks another with `--device`. Most people run one simulator at a time, each booted simulator costs several GB of memory, and a per-agent mapping would add the pane-ownership bookkeeping (`pane.closed` and `pane.moved` hooks) the web browser needs. If two agents in two projects use it at once they share the device, so per-agent devices are the upgrade path if that becomes common.
