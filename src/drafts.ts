export interface DraftController {
  id: string
  label: string
  isDirty: () => boolean
  save: () => Promise<boolean>
  discard: () => void
}

const registry = new Map<string, DraftController>()

export function registerDraft(controller: DraftController) {
  registry.set(controller.id, controller)
}

export function unregisterDraft(id: string) {
  registry.delete(id)
}

export function dirtyDrafts(): DraftController[] {
  return [...registry.values()].filter((controller) => {
    try {
      return controller.isDirty()
    } catch {
      // A broken dirty check must keep the exit guard closed.
      return true
    }
  })
}

export async function saveDirtyDrafts(): Promise<boolean> {
  for (const controller of dirtyDrafts()) {
    try {
      if (!(await controller.save())) return false
    } catch {
      return false
    }
  }
  return dirtyDrafts().length === 0
}

export function discardDirtyDrafts(): boolean {
  let failed = false
  for (const controller of dirtyDrafts()) {
    try {
      controller.discard()
    } catch {
      failed = true
    }
  }
  return !failed && dirtyDrafts().length === 0
}
