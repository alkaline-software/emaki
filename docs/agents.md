# Agents

About `crates/emaki-core/src/agents.rs` and
`crates/emaki-app/src/agents_page.rs`: the catalogue of coding agents,
looking for them on the machine, the agents page, and the agents' card in
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
`adapters` are the two whose sessions are read. `Agent::reads` joins
them. An agent with no adapter gets a card and a page, and its page says
its sessions are not read. A new adapter sets `reads` on its entry and
nothing else here changes.

## Looking

`detect_all` looks for every agent on a thread each, since each waits on
a program: the program by name in `search_dirs` (`PATH`, which `main.rs`
has already made the login shell's, then the folders installers and
version managers use), its version (`version_from` takes the dotted
number out of whatever is printed), and the sign-in.

The window looks once at launch, when the agents page is gone to and what
it has is older than `AGENTS_FRESH`, at "Check again", and whenever the
window becomes the active one with the agents page showing: that is what
a person does after installing something in a terminal.

## The page

Two levels, as the sessions page has (`Workbench::agent_open`).

- The cards: the installed under one head and the rest under another, in
  the catalogue's order. A card says the maker, what the agent is, and at
  its foot the version, a sign-in and the sessions kept, or "Set up".
- Inside one: three steps on a rail, each with a tick once done. Install
  shows where the program is, or the ways to get it for the system
  chosen (`agent_os`, this machine's to begin with), the maker's first
  choice first. Sign in shows what to type and what happens then, what
  account it takes, and the key variable. The third says whether Emaki
  reads its sessions. Under the steps, for an agent that is read, its
  projects: a click goes to the sessions page inside that project,
  narrowed to the agent.

## The sidebar's card

The card's head goes to the page. Its rows are the agents installed, and
any with sessions kept though the program is gone; a row goes inside that
agent. "Add an agent" under them goes to the cards, and is there while
the catalogue holds one that is not installed.

## Marks

`agent_mark`: Claude's own, then a file in `assets/icons/agents/` for
the agents in `MARKS` (Simple Icons' drawings of the makers' marks, CC0),
then the first letter of the name. Simple Icons' "amp" is another
product's and its Alibaba and AWS marks are the companies', not Qwen's or
Kiro's, so those agents wear a letter.

## Probing

- `emaki-core agents` prints what looking finds.
- `EMAKI_PAGE=agents` lands on the page. `EMAKI_GO=page:agents`,
  `agent:<id>` (inside one), `agentos:mac|linux|windows` and
  `agents:check` are the clicks.
