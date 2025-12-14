import React from "react";

const TailwindExamples: React.FC = () => {
  return (
    <div className="p-8 bg-slate-50 min-h-screen space-y-12">
      <section className="space-y-4">
        <h2 className="text-2xl font-bold text-slate-800">
          Advanced Tailwind CSS v3 Examples
        </h2>

        {/* 1. Gradients & Text Clip */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">
            Gradient Text & Backgrounds
          </h3>
          <div className="text-5xl font-extrabold">
            <span className="bg-clip-text text-transparent bg-gradient-to-r from-pink-500 to-violet-500">
              Hello Tailwind v3
            </span>
          </div>
          <div className="mt-4 flex gap-4">
            <button className="px-6 py-2 rounded-full bg-gradient-to-r from-cyan-500 to-blue-500 text-white font-semibold hover:from-cyan-600 hover:to-blue-600 transition-all shadow-lg shadow-cyan-500/50">
              Gradient Button
            </button>
            <button className="btn-primary-t animate-in fade-in zoom-in duration-1000 border-0">
              @apply Button (Animated)
            </button>
          </div>
        </div>

        {/* 2. Arbitrary Values (JIT) */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">Arbitrary Values (JIT)</h3>
          <div className="flex gap-4">
            <div className="w-[120px] h-[120px] bg-[#bada55] rounded-[30px] flex items-center justify-center text-white shadow-[0_35px_60px_-15px_rgba(0,0,0,0.3)]">
              w-[120px]
            </div>
            <div className="text-[2rem] leading-[1.1] text-[#333]">
              Arbitrary Font Size
            </div>
          </div>
        </div>

        {/* 3. Group & Peer Modifiers */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">Group & Peer Modifiers</h3>

          {/* Group Hover */}
          <div className="group block max-w-xs mx-auto rounded-lg p-6 bg-white ring-1 ring-slate-900/5 shadow-lg space-y-3 hover:bg-sky-500 hover:ring-sky-500 transition cursor-pointer">
            <div className="flex items-center space-x-3">
              <h3 className="text-slate-900 group-hover:text-white text-sm font-semibold">
                New Project
              </h3>
            </div>
            <p className="text-slate-500 group-hover:text-white text-sm">
              Create a new project from a variety of starting templates.
            </p>
          </div>

          {/* Peer */}
          <div className="mt-8">
            <input
              type="email"
              className="peer mt-1 block w-full px-3 py-2 bg-white border border-slate-300 rounded-md text-sm shadow-sm placeholder-slate-400
              focus:outline-none focus:border-sky-500 focus:ring-1 focus:ring-sky-500
              invalid:border-pink-500 invalid:text-pink-600
              focus:invalid:border-pink-500 focus:invalid:ring-pink-500
            "
              placeholder="you@example.com"
            />
            <p className="mt-2 invisible peer-invalid:visible text-pink-600 text-sm">
              Please provide a valid email address.
            </p>
          </div>
        </div>

        {/* 4. Filters & Backdrop Blur */}
        <div className="relative p-6 bg-white rounded-xl shadow-lg overflow-hidden">
          <h3 className="text-lg font-semibold mb-2">
            Filters & Backdrop Blur
          </h3>
          <div className="absolute inset-0 bg-[url('https://images.unsplash.com/photo-1554629947-334ff61d85dc?ixid=MnwxMjA3fDB8MHxwaG90by1wYWdlfHx8fGVufDB8fHx8&ixlib=rb-1.2.1&auto=format&fit=crop&w=1000&q=80')] bg-cover bg-center opacity-50"></div>
          <div className="relative flex justify-center items-center h-40 gap-4">
            <div className="w-24 h-24 bg-white/30 backdrop-blur-md rounded-lg flex items-center justify-center text-slate-800 font-bold shadow-xl border border-white/20">
              Blur
            </div>
          </div>
        </div>

        {/* 5. Animations & Transforms */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">
            Animations & Transforms
          </h3>
          <div className="flex gap-8 justify-center items-center">
            <div className="w-16 h-16 bg-blue-500 rounded-lg animate-bounce flex items-center justify-center text-white">
              Bounce
            </div>
            <div className="w-16 h-16 bg-green-500 rounded-lg animate-spin flex items-center justify-center text-white">
              Spin
            </div>
            <div className="w-16 h-16 bg-purple-500 rounded-lg hover:scale-125 hover:rotate-45 transition-transform duration-300 flex items-center justify-center text-white cursor-pointer">
              Hover
            </div>
          </div>
        </div>

        {/* 6. Aspect Ratio & Columns */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">Aspect Ratio & Columns</h3>
          <div className="columns-3 gap-4 space-y-4">
            <div className="w-full aspect-video bg-slate-200 rounded-lg flex items-center justify-center">
              Video 16:9
            </div>
            <div className="w-full aspect-square bg-slate-300 rounded-lg flex items-center justify-center">
              Square 1:1
            </div>
            <div className="w-full h-32 bg-slate-400 rounded-lg"></div>
            <div className="w-full aspect-[4/3] bg-slate-200 rounded-lg flex items-center justify-center">
              4:3
            </div>
          </div>
        </div>

        {/* 7. @apply Directive */}
        <div className="p-6 bg-white rounded-xl shadow-lg">
          <h3 className="text-lg font-semibold mb-2">@apply Directive</h3>
          <p className="text-slate-500 mb-4">
            The button below uses a custom class <code>.-t</code> defined in CSS
            using <code>@apply</code>.
          </p>
          <button className="btn-primary-t">Click me</button>
        </div>
      </section>
    </div>
  );
};

export default TailwindExamples;
