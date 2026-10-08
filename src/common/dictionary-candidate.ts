/** Owned dictionaries are maps nested on an owner, not CRUD entities. */
export const isDictionaryCandidate = (candidate: {
  inherits?: string;
}): boolean => candidate.inherits === "dictionary";
