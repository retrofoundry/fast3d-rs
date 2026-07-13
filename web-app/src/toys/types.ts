export type Owner = { id: string; handle: string };

/** Metadata returned by repository.list() — NO source (a future API lists summaries, fetches source on open). */
export type ToySummary = {
  slug: string; // stable, reserved; used in the URL hash (#t=<slug>)
  id: string; // server-minted later; equals slug for static toys
  title: string;
  description: string;
  owner: Owner;
  category: string;
  tags: string[];
  texture?: string; // bundled asset URL; omitted ⇒ the white default
  schemaVersion: 1;
};

/** A full toy (summary + the gbi source), returned by repository.get(). */
export type Toy = ToySummary & { source: string };

/** The editor's transient working copy — distinct from a persisted Toy. */
export type Draft = { source: string; title: string; description: string; forkOf?: string };

export const OFFICIAL_OWNER: Owner = { id: "official", handle: "official" };
