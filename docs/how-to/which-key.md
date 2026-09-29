<!-- @Yogesh(gap): page title; the page maps each key to the question you'd press it for -->
#

<!-- @Yogesh(gap): opening, one or two lines: when a reader opens this page (several keys look alike: g, G, M-g and b all list things to jump to) -->

<!-- @Yogesh(gap): one line saying the keys are the ones docs/tmux.conf.full.example binds, and that "Help says" is the command's --help line -->

<!-- @claude(note): the question cells are empty until you fill them; the table renders broken while the markers sit between rows, they come out with the answers -->

| Key | Runs | Help says | Answers |
| --- | --- | --- | --- |
<!-- @Yogesh(gap): question for g; it lists every pane, agents or not, with the last lines of each screen -->
| `prefix g` | `panes` | Every pane on the server, with what it runs and whether it has gone quiet; enter jumps there | |
<!-- @Yogesh(keep): question for G; in chat you picked "Where's the agent working on X?" -->
| `prefix G` | `panes --agents` | `--agents`: Only the panes running one of `[agents] programs` | |
<!-- @Yogesh(keep): question for M-g; in chat you picked "Who needs an answer from me?" -->
| `prefix M-g` | `inbox` | The agents waiting on you, each with the question it asked; enter jumps there | |
<!-- @Yogesh(gap): question for b; it's the inbox plus health reasons, idle sessions and numbers, and it opens itself on attach when there's news -->
| `prefix b` | `brief` | What needs you, on one screen: the agents waiting and what they asked, the health reasons, the sessions idle for days, the numbers | |
<!-- @Yogesh(gap): question for J; it looks back over the day rather than at now -->
| `prefix J` | `journal` | What happened in each project: the commands that ran long, the questions the agents asked, the sessions opened and closed | |
<!-- @Yogesh(gap): question for /; it searches every pane, where tmux's own copy-mode search stays in one -->
| `prefix /` | `search` | Every line of every pane's scrollback, newest first; enter goes to the pane and the line | |
<!-- @Yogesh(gap): question for P; the "address already in use" moment -->
| `prefix P` | `ports` | The ports something is listening on and the pane that started each; enter jumps there | |
<!-- @Yogesh(gap): question for M-s; a whole project session, against c which is one window -->
| `M-s` | `project` | Switch to a project, or start one | |
<!-- @Yogesh(gap): question for c; a window in this session at any directory -->
| `prefix c` | `new-window` | Open a new window, here or at any directory, from the directory picker | |
<!-- @Yogesh(gap): question for e -->
| `prefix e` | `run` | Run a command from history in a pane beside this one | |
<!-- @Yogesh(gap): question for ?; it searches and runs, against C-c which only shows -->
| `prefix ?` | `keys` | Searchable key bindings | |
<!-- @Yogesh(gap): question for C-c -->
| `prefix C-c` | `cheatsheet` | A cheat sheet of the bindings you wrote, in four boxes | |
<!-- @Yogesh(gap): question for z; zoom with other panes, the status bar when alone -->
| `prefix z` | `zen` | Clear everything but the pane you are working in | |
<!-- @Yogesh(gap): question for `; the pane comes back with its process and scrollback -->
| ``prefix ` `` | `pocket` | A shell that slides out beside this pane and is put away, process and scrollback kept, by the same key | |
<!-- @Yogesh(gap): question for @; refused when the pane's directory names the session it's already in, which is the message you hit today -->
| `prefix @` | `promote` | Give a pane a session of its own, named for the directory it is in | |
<!-- @Yogesh(gap): question for K; against tmux's kill-pane on x, the scrollback stays -->
| `prefix K` | `kill` | Stop what runs in front in a pane: TERM, and KILL if it is still there after the grace | |
<!-- @Yogesh(gap): question for X; quits nvim properly so no swap files are left -->
| `prefix X` | `project close` | Close this project by letting every window exit | |
<!-- @Yogesh(gap): question for S and M-S together? S pins this session's shape as the project's layout, M-S puts [[layout]] back in charge -->
| `prefix S` | `project save` | Capture this session's windows and panes as this project's layout | |
| `prefix M-S` | `project forget` | Delete this project's saved layout and fall back to the config | |
<!-- @Yogesh(gap): question for prefix M-s; every session on the server, where S is one project's shape -->
| `prefix M-s` | `sessions save` | Capture every session now, as a new generation | |
<!-- @Yogesh(gap): question for Q -->
| `prefix Q` | `quiet` | Quiet hours: no notifications, no nudges and no agent count on the bar for a while; the health mark says `quiet` instead | |
<!-- @Yogesh(gap): question for copy-mode o -->
| copy mode `o` | `open` | Open a URL or file found in text | |
