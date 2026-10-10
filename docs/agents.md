# Agents

About `crates/emaki-core/src/agents.rs` and
`crates/emaki-app/src/agents_page.rs`: the catalogue of coding agents,
looking for them on the machine, the agents page, and its entry in
the sidebar.

## Rules

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
  shows where the program is, or the ways to get it for the system
  chosen (`agent_os`, this machine's to begin with), the maker's first
  choice first. Sign in shows what to type and what happens then, what
  account it takes, and the key variable. The third says whether Emaki
  reads its sessions. Under the steps, for an agent that is read, its
  projects: a click goes to the sessions page inside that project,
  narrowed to the agent.

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
  `agent:<id>` (inside one), `agentos:mac|linux|windows` and
  `agents:check` are the clicks.
