import { Workspace } from "./components/Workspace";

function App() {
  return (
    <div className="min-h-screen bg-slate-950 text-slate-100">
      <div className="mx-auto flex h-screen max-w-5xl flex-col px-4 py-5">
        <Workspace />
      </div>
    </div>
  );
}

export default App;
