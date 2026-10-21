import { QueryClient } from '@tanstack/react-query'

type RootBeforeLoadFn = (args: {
  location: { pathname: string; search: Record<string, unknown> }
  context: { queryClient: QueryClient }
}) => Promise<unknown>

/**
 * Invoke the root route's `beforeLoad` directly. Tests that exercise the
 * loader mock `createRootRouteWithContext` to return the raw config, so
 * `beforeLoad` sits at the top level of the exported Route object — this
 * helper owns that bridge so the loader's argument shape is stubbed in one
 * place when the signature changes.
 */
export function runRootBeforeLoad(
  route: unknown,
  pathname: string,
  queryClient: QueryClient = new QueryClient()
): Promise<unknown> {
  const beforeLoad = (route as { beforeLoad?: RootBeforeLoadFn }).beforeLoad
  if (!beforeLoad) throw new Error('Route object does not expose beforeLoad (mock shape changed?)')
  return beforeLoad({ location: { pathname, search: {} }, context: { queryClient } })
}
