import { useId, useState } from 'react';
import type { ExpertQuestionData } from './client';

export function ExpertQuestion({ question, active, busy, onAnswer }: {
  question: ExpertQuestionData;
  active: boolean;
  busy: boolean;
  onAnswer: (answerIds: string[]) => void;
}) {
  const [selected, setSelected] = useState<string[]>([]);
  const hintId = useId();
  return <form className="expert-question" aria-label={question.title} onSubmit={event => {
    event.preventDefault();
    if (active && !busy && selected.length) onAnswer(selected);
  }}>
    <fieldset disabled={!active || busy} aria-describedby={hintId}>
      <legend>{question.title}</legend>
      {question.reason && <p>{question.reason}</p>}
      <p id={hintId} className="question-hint">Válassz egy vagy több lehetőséget, majd küldd el a választ.</p>
      <div className="question-options">{question.options.map((option, index) => <label key={option.id} className="question-option">
        <input type="checkbox" value={option.id} checked={selected.includes(option.id)} onChange={event => {
          setSelected(current => event.target.checked ? [...current, option.id] : current.filter(id => id !== option.id));
        }} />
        <span className="question-letter" aria-hidden="true">{String.fromCharCode(65 + index)}</span>
        <span><strong>{option.label}</strong>{option.description && <small>{option.description}</small>}</span>
      </label>)}</div>
      <button type="submit" className="button primary" disabled={!selected.length}>{busy && active ? 'Válasz küldése…' : 'Válasz elküldése'}</button>
    </fieldset>
    {!active && <p className="question-hint">Ez a kérdés már nem aktív.</p>}
  </form>;
}
