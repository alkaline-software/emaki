# Agents

About `crates/emaki-core/src/agents.rs` and
`crates/emaki-app/src/agents_page.rs`: the catalogue of coding agents,
looking for them on the machine, the agents page, and its entry in
the sidebar.

## Rules

- Words drawn by `selectable` that come and go have an id of their own
  (`selectable_as`), and nothing with an id is wrapped around them for
  a while and taken off again. The markdown view keeps what it parsed
  under its id and the ids of everything it stands in, and parses off
  the main thread: under a new id its words are blank for a frame. An
  animation's element has an id, so the install step's change is drawn
  off the clock in plain boxes. Wrapped in an animation, its words
  blinked when it began and again when it ended.

- Nothing here installs an agent, signs in to one, or runs a command the
  page shows. The person pastes it into a terminal of their own. An
  install needs what a new machine may lack, asks for a password, and
  changes with each maker's release.
- No file of an agent's is opened to look for it, a credential least of
  all. A sign-in is seen by a file being there, by a keychain item's
  attributes (never its secret, so the system asks nothing), or by a key
  in the environment.
- A sign-in not seen is not "signed out". Most agents can keep one in the
  system's keyring, where it cannot be seen without asking. The window
  says "signed in" or says nothing.
- A settings folder is not an installed agent. An editor's extension
  leaves `~/.copilot` with no `copilot` program. Installed means the
  program was found.
- A command in the catalogue is one its maker's documentation gives, word
  for word. One that could not be checked there is left out, and the page
  sends the person to the documentation for that system.
- Looking runs each program with its version flag and nothing else: no
  model is called, from the window or from a test.

## Two lists

`agents::all()` is every agent the window can speak of; `AgentId` and
`adapters` are the ones whose sessions are read, which today is both.
`Agent::reads` joins them. An agent with no adapter gets a card and a page, and its page says
its sessions are not read. A new adapter sets `reads` on its entry and
nothing else here changes.

The catalogue is two on purpose: Claude Code and Codex. Twelve were
listed at first, and nine were cut: a card for an agent whose sessions
are not read promised more than the app does. Gemini CLI was the third
and was cut too, once Google closed it to personal accounts (June 2026):
its sign-in fails for most people, and a saved sign-in file says nothing
of whether Google still serves the account.

## Beside the agents

`agents::tools()` is what is worth having with the agents and is not
one: GitHub Desktop, today. It is an `Agent` in every way the page
needs (a card, a page of steps, looked for with the rest, found by
`by_id`) and is in no list of agents: `all()` does not have it, so a new
session cannot be given to it. What sets it apart is in four fields:

- `apps`: where the app is installed, a path a system. It has a window
  and no program on `PATH`, so it is found there (`find_in`) and never
  run to be asked its version, which would open it.
- `download`: where its installer is got, shown as a button before the
  commands, which are the other way.
- `with_emaki`: what having it does for Emaki, in place of the step
  about sessions. It says GitHub Desktop's sign-in does not reach git,
  as the sign-in sheet does (`docs/panels.md`).
- `accent`: the accent of `look::ACCENTS` it wears where an agent wears
  its own colour (`agent_ink`): violet, GitHub Desktop's purple.

Its sign-in is seen as any is, by its keychain item's name on a Mac.
Elsewhere none is seen and the line says so.

## Looking

`detect_all` looks for every agent on a thread each, since each waits on
a program: the program by name in `search_dirs` (`PATH`, which `main.rs`
has already made the login shell's, then the folders installers and
version managers use), its version (`version_from` takes the dotted
number out of whatever is printed), and the sign-in.

The window looks once at launch, when the agents page is gone to and what
it has is older than `AGENTS_FRESH`, at the refresh button, and whenever the
window becomes the active one with the agents page showing: that is what
a person does after installing something in a terminal.

## Whether a command still works

The commands are part of the build and change only with a release. What
can be asked between releases is whether the thing a command fetches is
still published (`agents::stale_commands`, `source_of`): the script's
address, Homebrew's page for the formula or cask, npm's or PyPI's for
the package. A host that answers "not found" marks the command on the
agent's page; no network, or a host that will not say, marks nothing.
It cannot tell that a maker now recommends another way. The network is
asked at the refresh button, and otherwise on a visit to the page when the
last answer is a day old; never at launch.

## The page

Two levels, as the sessions page has (`Workbench::agent_open`).

- Under the agents' row, a second headed "Beside the agents": a
  tool's card (`tool_card`), low and as wide as the row over it, so the
  page has no empty half.
- The cards: side by side in the catalogue's order, whatever is
  installed, so a card stays where it is when that changes. The head is
  the name and the refresh button and nothing else; a line under it
  appears only when an install command's source is gone. A card is the
  mark, the name, the maker, what the agent is, then three lines of where
  it stands (the program and its version, a sign-in, the sessions kept),
  each with a tick when it is so, and at its foot "Set up" in the accent
  until it is installed and signed in, "Details" after. The sign-in line
  without a tick says "No sign-in seen", never "signed out".
- Inside one, the page's words can be selected and copied as a
  conversation's can (`Workbench::selectable`: each is drawn by the
  markdown view with markdown's marks escaped, and a right click is the
  conversation's own menu). The agents' names and the cards are not.
- Inside one, a button at the strip's right opens a shell in the home
  folder at the page's right, for the commands the page gives: the
  conversation's terminal panel with its shell side alone
  (`docs/channels.md`, The terminal panel).
- Inside one: three steps on a rail, each with a tick once done. Install
  shows where the program is, and two choices on one row with a
  chevron between, read as a path: the system (`agent_os`, this
  machine's to begin with), then the way on it (`agent_way`: a download
  where there is one, and each command), the maker's first choice to
  begin with. Under the row, what the way says to do. Tried and
  dropped: the ways in a bordered box under the system, which read as
  one border too many and set the two controls out of line. A change of
  either choice is in motion (`agent_choose`, `AgentSwap`): what was
  there fades out where it stood as the new fades in, and their room
  runs from the old height to the new, measured as it is drawn, so the
  steps under it slide. One way shows at a time, since
  they are one or the other. The way's control has an id a system, so
  its plate does not slide between two systems' lists. Each choice has
  an icon before its word (`choice_icon`, by the control and the key):
  a system's mark, and for a way what does the installing. The penguin
  is drawn here; the rest are Tabler's. Sign in shows what to type and what happens then, what
  account it takes, and the key variable. What to do stays under a step
  that is done: for another machine, or to do it again.
- A command's words are escaped down to the colon (`selectable`): the
  markdown view made a link of an address in a command and drew the
  escapes inside it. The third says whether Emaki
  reads its sessions. The sessions themselves are not listed here: All
  Projects has them, and narrows to an agent. A list of an agent's
  projects stood under the steps and was removed for saying it twice.

## The sidebar

"Agents" is an entry under All Projects (⌘E, `GoAgents`), which goes to
the cards. There was a card of agents in the sidebar, a row an agent
with its sessions' count; it was removed for the entry, since the page
says the same and more.

## Marks

`agent_mark`: Claude's own, then a file in `assets/icons/agents/` for
the agents in `MARKS` (Simple Icons' drawings of the makers' marks, CC0),
then the first letter of the name, for an agent added with no mark yet.

A mark's colour is its agent's and never the accent's: Claude's
terracotta, Codex's green (`workbench::agent_color`, the two accents of
those names in `look::ACCENTS`, each with its shade for the appearance).
The words beside a mark that say the agent is at work wear the same.
Two places keep a mark grey until there is a reason: an idle session's
row, and the terminal button in the top strip until it is pointed at or
its side shows. A folder is not a mark and follows the accent. The list
of agents on a new session has the mark before each name, and the
maker's line under it starts where the name does.

On the agents page everything of an agent's that would wear the accent
or the green of "done" wears the agent's colour instead (`agent_ink`):
the ticks, the "Installed" chip, "Set up", a card's border under the
pointer, the pills' wash, the tick of a command copied. An agent whose
sessions are not read has no colour and keeps the accent.

## Probing

- `emaki-core agents` prints what looking finds.
- `EMAKI_PAGE=agents` lands on the page. `EMAKI_GO=page:agents`,
  `agent:<id>` (inside one), `agentos:mac|linux|windows`,
  `agentway:<name>` (a way as the catalogue names it, or `Download`)
  and `agents:check` are the clicks.
