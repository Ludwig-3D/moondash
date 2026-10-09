/** Resolve AFC's full unit IDs (e.g. "Box_Turtle Turtle_1") to their
 * Klipper object keys (e.g. "AFC_BoxTurtle Turtle_1"). */
export type AfcUnitEntry = {
  id: string
  key: string
  type: string
  name: string
  data: Record<string, any>
  lanes: string[]
}

type Objects = Record<string, any>

export function discoverAfcUnits(objects: Objects): AfcUnitEntry[] {
  const root = objects?.AFC
  if (!Array.isArray(root?.units)) return []

  return root.units.flatMap((raw: unknown): AfcUnitEntry[] => {
    if (typeof raw !== 'string' || !raw.trim()) return []
    const fullId = raw.trim()
    const space = fullId.indexOf(' ')
    const type = space >= 0 ? fullId.slice(0, space) : ''
    const name = space >= 0 ? fullId.slice(space + 1) : fullId
    // AFC uses Box_Turtle in its list, but AFC_BoxTurtle in object names.
    const normalizedType = type.replace(/_/g, '')
    const candidates = [
      `AFC_${normalizedType} ${name}`,
      `AFC_${type} ${name}`,
      `AFC_${fullId}`,
    ]
    const key = candidates.find((candidate) => objects[candidate] != null)
      ?? Object.keys(objects).find((candidate) =>
        candidate.startsWith('AFC_') &&
        candidate.slice(candidate.indexOf(' ') + 1) === name &&
        candidate.slice(4, candidate.indexOf(' ')).replace(/_/g, '').toLowerCase() === normalizedType.toLowerCase(),
      )
    if (!key) return []
    const data = objects[key]
    const lanes = Array.isArray(data?.lanes)
      ? data.lanes.filter((lane: unknown): lane is string => typeof lane === 'string')
      : Object.entries(objects)
          .filter(([k, value]) => k.startsWith('AFC_stepper ') && value?.unit === name)
          .map(([k]) => k.slice('AFC_stepper '.length))
    return [{ id: name, key, type, name, data, lanes }]
  })
}
