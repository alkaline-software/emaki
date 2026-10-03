# Emaki: stages

Emaki is a desktop app for people who use coding agents such as Claude Code
and Codex. It depends on those agents being installed, and it adds three
things on top of them: conversations that never expire, one place to
organize them, and a readable view of what the agent did.

This file is the high-level plan, in three stages. PLAN.md is the detailed
one: the tasks, the decisions and what has landed.

## Stage 1: A complete app for Claude Code (current)

- **Onboarding.** A first-run window presents the mainstream agents (Claude
  Code, Codex, Kimi and others) as cards. Clicking a card takes the user to
  the official site to install it, since Emaki cannot do anything without
  at least one agent. Claude Code is the supported one; the other cards say
  that support is coming soon.
- **Permanent history.** Every conversation is saved into Emaki's own
  folder, so it never expires. The user decides what to rename, collect or
  delete.
- **Organization.** Conversations can be browsed three ways: by recency, by
  project, and by agent.
- **Readable conversations.** Agent replies are rendered as Markdown
  instead of plain text, and tool calls are categorized and listed.
- **Explanations.** Each step has a button that asks a small model (Haiku)
  to explain, at a high level, what the agent is doing.

## Stage 2: Public release and feedback

Once Emaki works fully with Claude Code, release it to the public,
advertise it to Claude Code users, and collect their feedback.

## Stage 3: Support for other agents

Extend the same features to Codex, Kimi and other mainstream coding agents,
so Emaki works across all of them.
