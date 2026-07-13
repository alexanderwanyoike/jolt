# Application System

## Overview

Jolt apps are portable interfaces for Jolt spaces.

The protocol owns the hard parts: identity, signed state, content addressing, access control, relays, provider discovery, and peer caching. Apps sit above that substrate and give a space a useful experience.

Examples:

- A game community app renders releases, mods, announcements, lobbies, and matchmaking.
- A creator app renders member feeds, media, comments, and recommendations.
- A research app renders datasets, notebooks, citations, and usage rights.
- A legal workspace app renders documents, signatures, versions, and evidence graphs.

Apps are external clients the user runs and controls. They hold no keys and
get no ambient authority: an app requests a capability-scoped session from the
local daemon, the user approves it in Jolt Console, and every call the app
makes is checked against that grant. The core product is owned community state
that any authorized client can verify; the app is a replaceable view over it.

## Near-Term App Boundary

The first real app proof is not the WASM runtime. Pastey proved a nearer boundary:

```text
external app -> local Jolt daemon -> Jolt network
```

In this model, the daemon is the local authority for identities, keys, settings, content, and network access. Apps are untrusted clients that request scoped sessions. Jolt Console is the privileged local control surface where the user approves and revokes app grants.

The session model is defined in [App Boundary and Sessions](15-app-boundary-and-sessions.md). An earlier design distributed apps as sandboxed WASM packages installed onto the node; that direction was abandoned in favor of this external-app boundary.

### What Is Implemented Today

The implemented app model already covers more than publish/fetch. Through
capability-checked `/app/v1/*` endpoints, an approved external app can:

- resolve public `.jolt` addresses and fetch public content;
- publish signed path updates and append records under granted path scopes;
- enumerate append records (own identity or, with an explicit grant, any
  identity) within its approved namespace;
- publish, decrypt, open, and rewrap encrypted objects (doc 16);
- send, list, accept, reject, and open recipient-ingress items;
- list its published inventory and request home-relay pins.

The exact endpoint list, capability grammar, and session lifecycle are in
doc 15. [Spoke](https://github.com/alexanderwanyoike/spoke) is the working
example app built on this boundary.

## Role in the Stack

```text
Jolt Protocol
  identity, CIDs, update logs, encryption, relays, access

Jolt Space
  signed community state: members, feeds, content refs, permissions

Jolt App
  renderer/editor/tool for a kind of space
```

The app should receive capability-limited access to the space. It should not become the authority over the space.

## Protocol Boundary

Apps sit above the protocol. The protocol should stay pure and durable: identity, CIDs, signed update logs, provider discovery, content fetch, relays, pinning, encryption/access grants, capability records, schema references, and generic signed paths.

The protocol must not hardcode application concepts such as profiles, feeds, posts, galleries, games, timelines, or lens runtimes. Those are signed content and schemas interpreted by clients.

```text
Protocol:
  identity X maps path /gallery to CID Y at sequence N

Application/lens:
  CID Y is a gallery manifest and this renderer knows how to use it
```

This keeps Jolt closer to the web's layering discipline: the lower layer moves verifiable state and addressing, while higher layers decide what experiences to build from it.

## HTML as a Space View

HTML remains a valid interface format for browsing a space.

The important distinction is:

```text
Signed space state = authority
HTML = view
```

A space may publish a generated HTML tree for easy browsing, linking, and media layout. A client should still verify the signed records and content IDs that produced the view. This gives Jolt a familiar browseable surface without reducing the protocol to "web pages on P2P".
