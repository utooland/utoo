import { Checkbox, Input } from "antd";
import React from "react";
import styled from "styled-components";
import { proxy, snapshot, subscribe, useSnapshot } from "valtio";

export function proxyWithPersist<V extends Object>(
  val: V,
  opts: {
    key: string;
  },
) {
  const local = localStorage.getItem(opts.key);
  const state = proxy<V>(local ? JSON.parse(local) : val);
  subscribe(state, () => {
    const snapshotState = snapshot(state);
    localStorage.setItem(opts.key, JSON.stringify(snapshotState));
  });
  return state;
}

const todos = proxyWithPersist(
  {
    list: [{ id: 1, text: "hello world", done: false }],
  },
  {
    key: "todos",
  },
);

const actions = {
  toggleChecked(id: number) {
    for (const item of todos.list) {
      if (item.id === id) {
        item.done = !item.done;
      }
    }
  },
  addTodo(todo: { id: number; text: string; done: boolean }) {
    todos.list.push(todo);
  },
};

const Wrapper = styled.div`
	.list div {
		display: flex;
	}
	.input {
		margin-top: 1rem;
	}
`;

export function Todos() {
  const snap = useSnapshot(todos);
  const [text, setText] = React.useState("");
  return (
    <Wrapper>
      <h2>Todos</h2>
      <div className="list">
        {snap.list.map((todo) => {
          return (
            <div key={todo.id}>
              <Checkbox
                checked={todo.done}
                onChange={() => actions.toggleChecked(todo.id)}
              >
                {todo.text}
              </Checkbox>
            </div>
          );
        })}
      </div>
      <div className="input">
        <Input
          value={text}
          placeholder="What needs to be done?"
          onChange={(e) => {
            setText(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              text &&
                actions.addTodo({
                  id: Math.random(),
                  text,
                  done: false,
                });
              setText("");
            }
          }}
        />
      </div>
    </Wrapper>
  );
}
