# heckle guide for AI agents

heckle shows a Markdown file in a terminal pane the way a browser renders it, and lets the user
review it like a GitHub pull request: they leave comments on its blocks (headings, paragraphs,
lists, code blocks) and on the whole file. Those comments become conversations: you reply to
each one, edit the file, and the open pane follows. Everything is saved as the user goes, so you
never need the pane to be closed to read it.

The key rule: **heckle is interactive and runs in its own pane beside you.** Never run
`heckle review` or `heckle open` in your own shell. They refuse to start when their output isn't a
terminal. Opening a review never blocks you. Open the pane, tell the user, and carry on.

## 1. Open a review

1. Pick the file: a Markdown file the user wants to review (a plan, a spec, notes, a README).

2. Pick where the review is saved, so you know its path and id later:

   ```sh
   out="$HOME/.local/state/heckle/reviews/$(basename "$PWD")-$(date +%Y%m%d-%H%M%S)/review.md"
   heckle_bin="$(command -v heckle || echo "$HOME/.local/bin/heckle")"
   ```

   Pass `"$heckle_bin"` (the absolute path) to pane-splitting commands: they run without the
   user's interactive shell config, so `~/.local/bin` may not be on their PATH.

   The review id is printed at the top of `heckle result` (`<!-- heckle review ID: path -->`).
   An `--out` path that already holds a review of the same file opens that review again, with
   its conversations.

3. Open heckle in a new pane beside the conversation. It needs a terminal with Kitty graphics:
   - **Rex** (`$REX_SESSION` is set):
     `rex split -s "$REX_SESSION" -b "$REX_BLOCK" --split=right --ratio 60 --focus=false -c "$PWD" -- "$heckle_bin" review --out "$out" FILE.md`
   - **kitty** (with remote control on):
     `kitty @ launch --location=vsplit --cwd="$PWD" "$heckle_bin" review --out "$out" FILE.md`
   - **WezTerm**: `wezterm cli split-pane --right --cwd "$PWD" -- "$heckle_bin" review --out "$out" FILE.md`
   - Anything else (including tmux, zellij and Warp, which can't show its images): don't open it
     yourself. Ask the user to run `heckle review --out "$out" FILE.md` in a Kitty-graphics
     terminal (Rex, Ghostty, kitty, WezTerm, iTerm2).

4. Tell the user in one line, e.g. "Opened the review beside us. Tell me when you want me to
   pick it up." **Don't wait for or poll the review.** Nothing in heckle answers on its own:
   you answer when the user asks you to.

## 2. Pick it up

When the user says they're done (or asks you to look):

1. Read it: `cat "$out"`, or `heckle result` for the newest review (`heckle result --list`,
   `heckle result <id>`, `--json` for the structured form).
2. The `Status:` line says where it stands:
   - `finished`: the title has the verdict (approved / changes requested / comments).
   - `in progress`: the pane is still open and more comments may come. Say you're working
     from a partial review.
   - `closed without finishing`: the pane was closed before Finish. The comments still count.
3. `## Overall` is the user's comment on the whole file. Under `## Comments`, each one starts a
   numbered conversation (`Comment 3 on lines 12–15:`) and quotes the Markdown source it's on,
   so you don't need to look anything up. Replies follow as `You replied:` (the user) and
   `Claude replied:` (you; replies from any agent show as "Claude").
   - Answer the newest message in each conversation: edit the file, or answer the question.
   - `(resolved)`: settled. Skip it unless the user's last message asks for something.
   - Only conversations whose newest message is the user's need you. The rest you already
     answered.
   - `(outdated)`: the lines it was on changed since. It quotes them as they were. Check that
     your change covers it, and say so in your reply.

## 3. Reply inline, and edit the file

Answer each conversation where the user will see it, under their comment:

```sh
heckle reply <id> --to 3 "Split the step in two (lines 14–19)."
heckle reply <id> --overall "Thanks! Addressed everything except #4, see my reply there."
```

`--to N` is the comment's number in the review file. Keep replies short: what you changed and
where, or the answer. An open pane shows them within a second. Also give the user a short
summary in the conversation.

Edit the Markdown file itself to make the changes. The open pane reloads it and keeps each
conversation on its lines; conversations on lines you changed become outdated. There is no
refresh step.

## 4. Open it again

If the pane was closed, `heckle open <id>` (in a new pane, as in step 1) shows the review again
with its conversations.

## Keys (in case the user asks)

- **Comment:** `a`, or click the `+` in a block's left margin, comments on that block. `A`
  writes the overall comment.
- **Comment box:** Ctrl-S saves and Esc cancels. Tab steps to the Cancel and Comment buttons,
  and Enter presses one.
- **Conversations:** each has a Reply… box and Resolve conversation, and the user's own
  messages have Edit and Delete. Resolved and outdated ones fold to one line; Show opens them.
- **Finishing:** `F`, then `a` approve, `r` request changes, or `c` comment. `q` closes the
  pane; the review is already saved.
- **Reading:** `j`/`k`/Space scroll, `/` searches, `t` shows the contents, `+`/`-` zoom, and `?`
  lists every key.
