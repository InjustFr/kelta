// Dropped files (#524): their paths are pasted shell-escaped, as other terminals do.

/** Backslash-escapes the characters a POSIX shell would split or expand in a dropped path. */
export function shellEscape(path: string): string {
  return path.replace(/[ \\'"()[\]{}<>|;&*?$!#~`]/g, '\\$&');
}
