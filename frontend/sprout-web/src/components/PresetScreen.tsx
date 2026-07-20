import { useState, type FormEvent } from 'react'
import type { ThreePretaskPresetInput } from '../domain/presets'

export interface PresetMaterializationInput extends ThreePretaskPresetInput {
  templateAttachments: Partial<
    Record<'priority' | 'deadline' | 'recurring', File>
  >
}

interface PresetScreenProps {
  destinationReady: boolean
  result?: { id: string; name?: string; locked?: boolean; detail?: string }
  onMaterialize(input: PresetMaterializationInput): Promise<void>
}

const localDate = (days: number): string => {
  const value = new Date(Date.now() + days * 86_400_000)
  value.setSeconds(0, 0)
  return new Date(value.getTime() - value.getTimezoneOffset() * 60_000)
    .toISOString()
    .slice(0, 16)
}

export const PresetScreen = ({
  destinationReady,
  result,
  onMaterialize,
}: PresetScreenProps) => {
  const [name, setName] = useState('Three-task preset')
  const [priorityTitle, setPriorityTitle] = useState('Review priority')
  const [priority, setPriority] =
    useState<ThreePretaskPresetInput['priority']>('normal')
  const [deadlineTitle, setDeadlineTitle] = useState('Meet deadline')
  const [deadlineDueAt, setDeadlineDueAt] = useState(localDate(1))
  const [recurringTitle, setRecurringTitle] = useState('Recurring follow-up')
  const [recurringDueAt, setRecurringDueAt] = useState(localDate(2))
  const [frequency, setFrequency] =
    useState<ThreePretaskPresetInput['frequency']>('weekly')
  const [interval, setInterval] = useState(1)
  const [priorityFile, setPriorityFile] = useState<File>()
  const [deadlineFile, setDeadlineFile] = useState<File>()
  const [recurringFile, setRecurringFile] = useState<File>()

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    await onMaterialize({
      name,
      priorityTitle,
      priority,
      deadlineTitle,
      deadlineDueAt: new Date(deadlineDueAt).toISOString(),
      recurringTitle,
      recurringDueAt: new Date(recurringDueAt).toISOString(),
      frequency,
      interval,
      templateAttachments: {
        priority: priorityFile,
        deadline: deadlineFile,
        recurring: recurringFile,
      },
    })
  }

  return (
    <section className="content-screen" aria-labelledby="preset-title">
      <div className="screen-heading">
        <div>
          <p className="eyebrow">Preset lifecycle</p>
          <h2 id="preset-title">Preset, assignment and materialization</h2>
        </div>
      </div>
      <form className="panel-form" onSubmit={(event) => void submit(event)}>
        <p>
          Creates an immutable version with all three pretask kinds, assigns
          independently selected values, and materializes encrypted task
          snapshots in the selected task list.
        </p>
        <label>
          Preset name
          <input required value={name} onChange={(event) => setName(event.target.value)} />
        </label>
        <label>
          Priority pretask
          <input
            required
            value={priorityTitle}
            onChange={(event) => setPriorityTitle(event.target.value)}
          />
        </label>
        <label>
          Selected priority
          <select
            value={priority}
            onChange={(event) =>
              setPriority(event.target.value as ThreePretaskPresetInput['priority'])
            }
          >
            <option value="low">Low</option>
            <option value="normal">Normal</option>
            <option value="high">High</option>
          </select>
        </label>
        <label>
          Priority template attachment (optional)
          <input
            type="file"
            onChange={(event) => setPriorityFile(event.target.files?.[0])}
          />
        </label>
        <label>
          Deadline pretask
          <input
            required
            value={deadlineTitle}
            onChange={(event) => setDeadlineTitle(event.target.value)}
          />
        </label>
        <label>
          Deadline template attachment (optional)
          <input
            type="file"
            onChange={(event) => setDeadlineFile(event.target.files?.[0])}
          />
        </label>
        <label>
          Selected deadline
          <input
            required
            type="datetime-local"
            value={deadlineDueAt}
            onChange={(event) => setDeadlineDueAt(event.target.value)}
          />
        </label>
        <label>
          Recurring template attachment (optional)
          <input
            type="file"
            onChange={(event) => setRecurringFile(event.target.files?.[0])}
          />
        </label>
        <label>
          Recurring pretask
          <input
            required
            value={recurringTitle}
            onChange={(event) => setRecurringTitle(event.target.value)}
          />
        </label>
        <label>
          First occurrence
          <input
            required
            type="datetime-local"
            value={recurringDueAt}
            onChange={(event) => setRecurringDueAt(event.target.value)}
          />
        </label>
        <label>
          Frequency
          <select
            value={frequency}
            onChange={(event) =>
              setFrequency(
                event.target.value as ThreePretaskPresetInput['frequency'],
              )
            }
          >
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
            <option value="monthly">Monthly</option>
          </select>
        </label>
        <label>
          Interval
          <input
            required
            min={1}
            type="number"
            value={interval}
            onChange={(event) => setInterval(Number(event.target.value))}
          />
        </label>
        <button className="primary-button" type="submit" disabled={!destinationReady}>
          Create, assign and materialize
        </button>
        {!destinationReady && <small>Select a task list first.</small>}
      </form>
      {result && (
        <div className="result-card" role="status">
          <div>
            <strong>{result.name ?? 'Preset materialized'}</strong>
            <p>{result.detail ?? result.id}</p>
          </div>
        </div>
      )}
    </section>
  )
}
