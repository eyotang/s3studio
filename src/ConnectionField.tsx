import { useState } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import type { S3Connection } from "./types";

export default function ConnectionField({
  name,
  initialValue,
  placeholder,
  options,
  disabled,
  autoFocus,
  onAdd,
  onDelete,
}: {
  name: keyof S3Connection;
  initialValue: string;
  placeholder: string;
  options: string[];
  disabled: boolean;
  autoFocus?: boolean;
  onAdd: (value: string) => Promise<boolean>;
  onDelete?: (value: string) => void;
}) {
  const [value, setValue] = useState(initialValue);
  const title = name === "endpoint" ? "Endpoint" : "Region";
  const normalized =
    name === "endpoint" ? value.trim().replace(/\/+$/, "") : value.trim();
  return (
    <div className="connection-field">
      <label htmlFor={`s3-${name}`}>{title}</label>
      <div className="connection-control">
        <input
          id={`s3-${name}`}
          name={name}
          type={name === "endpoint" ? "url" : "text"}
          value={value}
          placeholder={placeholder}
          autoCapitalize="none"
          spellCheck={false}
          disabled={disabled}
          autoFocus={autoFocus}
          onChange={(event) => setValue(event.target.value)}
        />
        <div className="connection-picker">
          <ChevronDown size={14} aria-hidden="true" />
          <select
            aria-label={`选择已保存的 ${title}`}
            value={
              !normalized || options.includes(normalized) ? normalized : "*"
            }
            disabled={disabled}
            onChange={(event) => setValue(event.target.value)}
          >
            <option value="*" disabled hidden>
              选择已保存的值
            </option>
            <option value="">使用默认值</option>
            {options.map((option) => (
              <option key={option}>{option}</option>
            ))}
          </select>
        </div>
        {normalized && !options.includes(normalized) && (
          <button
            className="secondary-button"
            type="button"
            aria-label={`新增 ${title}`}
            disabled={disabled}
            onClick={async () => {
              if (await onAdd(normalized)) setValue(normalized);
            }}
          >
            新增
          </button>
        )}
      </div>
      {onDelete && options.length > 0 && (
        <ul className="connection-options" aria-label={`已保存的 ${title}`}>
          {options.map((option) => (
            <li key={option}>
              <span title={option}>{option}</span>
              <button
                type="button"
                aria-label={`删除选项 · ${option}`}
                disabled={disabled}
                onClick={() => {
                  onDelete(option);
                  if (normalized === option) setValue("");
                }}
              >
                <Trash2 size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
