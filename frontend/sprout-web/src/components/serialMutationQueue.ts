export type SerialMutationQueue = <Result>(
  mutation: () => Promise<Result>,
) => Promise<Result>

/**
 * Runs mutations strictly one at a time and keeps the queue usable when one
 * mutation fails. The returned promise still reports that individual failure.
 */
export const createSerialMutationQueue = (): SerialMutationQueue => {
  let tail: Promise<unknown> = Promise.resolve()

  return <Result>(mutation: () => Promise<Result>): Promise<Result> => {
    const result = tail.then(mutation, mutation)
    tail = result.then(
      () => undefined,
      () => undefined,
    )
    return result
  }
}
