// @ts-check
// herdr-ios-simulator menu (mod.open:herdr-ios-simulator): acts for the agent that had focus.
// Copied from herdr-web-browser's menu mod.
import { View, div } from "gpui-kit";
import { v_flex } from "gpui-base";
import { exec, agents, focused, props, closeOverlay, notify, theme } from "herdr";

const BIN = "__HERDR_IOS_BIN__";

export default class Simulator extends View {
  init(_props, _cx) {
    const f = focused();
    const list = agents();
    const agent = list.find((a) => a.address === f?.terminal) ?? list.find((a) => a.address === f?.agent);
    this.agent = agent ? agent.address : null;
    this.busy = null;
  }

  act(label, args, cx) {
    this.busy = label;
    cx.notify();
    cx.spawn(async (_cx) => {
      const r = await exec(BIN, args).catch((e) => ({ code: 1, stdout: "", stderr: String(e) }));
      const out = (r.code === 0 ? r.stdout : r.stderr).trim().slice(0, 240);
      notify({ title: "iOS Simulator", body: out || label, level: r.code === 0 ? "success" : "error" });
      closeOverlay(props().overlay);
    });
  }

  row(t, id, label, args) {
    return div().id(id).px(12).py(7).rounded(6).cursor_pointer()
      .hover((s) => s.bg(t.hover))
      .on_click((_e, cx) => this.act(label, args, cx))
      .child(label);
  }

  render(_cx) {
    const t = theme();
    const a = this.agent;
    const to = a ? ["--agent", a] : [];
    const rows = [
      this.row(t, "hi-open", "Open the simulator", ["boot", "--show"]),
      this.row(t, "hi-home", "Home", ["button", "home"]),
      this.row(t, "hi-shot", "Send screenshot to agent", ["send", "screenshot", ...to]),
      this.row(t, "hi-errors", "Send errors to agent", ["send", "errors", ...to]),
      this.row(t, "hi-agents", "Set up agents", ["install-agents"]),
      this.row(t, "hi-shutdown", "Shut the simulator down", ["shutdown"]),
    ];
    return v_flex().id("root").size_full().bg(t.surface).p(6).gap(1).text_size(13).text_color(t.foreground)
      .child(div().px(12).py(6).text_color(t.muted).child(this.busy ? `${this.busy}…` : a ? `iOS Simulator · ${a}` : "iOS Simulator · no agent focused"))
      .children(rows);
  }
}
