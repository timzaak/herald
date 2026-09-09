interface FilterableItem {
  permission?: string | string[] | null
  id?: string
  children?: FilterableItem[]
}

/**
 * An item passes when the admin holds any one of its required permissions;
 * a single string behaves exactly like a one-element array.
 */
function hasAnyRequiredPermission(item: FilterableItem, permissions: string[]): boolean {
  if (!item.permission) return true
  const required = Array.isArray(item.permission) ? item.permission : [item.permission]
  return required.some((permission) => permissions.includes(permission))
}

export function filterByPermission<T extends FilterableItem>(
  items: T[],
  permissions: string[],
  realmId?: string
): T[] {
  return items
    .filter((item) => {
      if (item.id === 'realms' && realmId !== undefined && realmId !== 'admin') return false
      if (!hasAnyRequiredPermission(item, permissions)) return false
      return true
    })
    .map((item) => {
      if (item.children && item.children.length > 0) {
        const filteredChildren = filterByPermission(item.children, permissions, realmId)
        return { ...item, children: filteredChildren } as T
      }
      return item
    })
    .filter((item) => {
      if (item.children && item.children.length === 0) return false
      return true
    })
}
