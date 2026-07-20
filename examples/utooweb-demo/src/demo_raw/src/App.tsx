import "./App.less";

import { Flex, Layout } from "antd";
import React, { useState } from "react";
import { getOrder } from "./apis/order.server";
import TailwindExamples from "./TailwindExamples";

const { Header, Footer, Sider, Content } = Layout;

function OrderServerFunctionDemo() {
  const [result, setResult] = useState("");

  async function handleQueryOrder() {
    const order = await getOrder("ORDER-1001");
    setResult(`${order.orderId}: ${order.status}`);
  }

  return (
    <section className="m-8 rounded-xl bg-white p-6 shadow-lg">
      <h2 className="text-xl font-bold">Server Function Order Demo</h2>
      <p className="my-3 text-slate-600">
        Query an order through the generated @evjs/client proxy.
      </p>
      <button
        type="button"
        className="rounded-lg bg-blue-600 px-4 py-2 text-white"
        onClick={handleQueryOrder}
      >
        Query demo order
      </button>
      {result && <p className="mt-3 text-green-700">{result}</p>}
    </section>
  );
}

const headerStyle: React.CSSProperties = {
  textAlign: "center",
  color: "#fff",
  height: 64,
  paddingInline: 48,
  lineHeight: "64px",
  backgroundColor: "#4096ff",
};

const contentStyle: React.CSSProperties = {
  textAlign: "center",
  minHeight: 120,
  lineHeight: "120px",
  color: "#fff",
  backgroundColor: "#0958d9",
};

const siderStyle: React.CSSProperties = {
  textAlign: "center",
  lineHeight: "120px",
  color: "#fff",
  backgroundColor: "#1677ff",
};

const footerStyle: React.CSSProperties = {
  textAlign: "center",
  color: "#fff",
  backgroundColor: "#4096ff",
};

const layoutStyle = {
  borderRadius: 8,
  overflow: "hidden",
  width: "calc(50% - 8px)",
  maxWidth: "calc(50% - 8px)",
};

const App: React.FC = () => (
  <div>
    <OrderServerFunctionDemo />
    <TailwindExamples />
    <Flex gap="middle" wrap>
      <Layout style={layoutStyle}>
        <Header style={headerStyle}>Header</Header>
        <Content style={contentStyle}>Content</Content>
        <Footer style={footerStyle}>Footer</Footer>
      </Layout>

      <Layout style={layoutStyle}>
        <Header style={headerStyle}>Header</Header>
        <Layout>
          <Sider width="25%" style={siderStyle}>
            Sider
          </Sider>
          <Content style={contentStyle}>Content</Content>
        </Layout>
        <Footer style={footerStyle}>Footer</Footer>
      </Layout>

      <Layout style={layoutStyle}>
        <Header style={headerStyle}>Header</Header>
        <Layout>
          <Content style={contentStyle}>Content</Content>
          <Sider width="25%" style={siderStyle}>
            Sider
          </Sider>
        </Layout>
        <Footer style={footerStyle}>Footer</Footer>
      </Layout>

      <Layout style={layoutStyle}>
        <Sider width="25%" style={siderStyle}>
          Sider
        </Sider>
        <Layout>
          <Header style={headerStyle}>Header</Header>
          <Content style={contentStyle}>Content</Content>
          <Footer style={footerStyle}>Footer</Footer>
        </Layout>
      </Layout>
    </Flex>
  </div>
);

export default App;
