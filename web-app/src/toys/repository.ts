import type { Toy, ToySummary } from "./types";
import { TOYS } from "./index";

export interface ToyRepository {
  list(): Promise<ToySummary[]>;
  get(slug: string): Promise<Toy | null>;
  // future: publish(draft), update(toy), delete(slug), listByOwner(ownerId) + auth
}

export class StaticToyRepository implements ToyRepository {
  async list(): Promise<ToySummary[]> {
    // Strip `source` — list returns summaries, mirroring a paginated API.
    return TOYS.map(({ source: _source, ...summary }) => summary);
  }
  async get(slug: string): Promise<Toy | null> {
    return TOYS.find((t) => t.slug === slug) ?? null;
  }
}

export const repository: ToyRepository = new StaticToyRepository();
