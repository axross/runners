// the order the main window's views come in. A view reaches the page as an event and as
// the reply to `read_view`, and nothing orders the two, so each carries the session's
// revision and the page keeps the newest it has drawn.

/**
 * returns a function that says whether a view of `revision` is to be drawn: it is unless
 * a newer one has been. A view of the revision already drawn is, since the event and the
 * reply to a read can carry one view.
 */
export function newestFirst() {
  let newest = 0;
  return (revision) => {
    if (revision < newest) {
      return false;
    }
    newest = revision;
    return true;
  };
}
