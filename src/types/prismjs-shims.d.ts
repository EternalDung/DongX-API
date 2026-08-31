// prismjs 是纯 JS 包，无自带类型；这里为其主模块及按需加载的语言/主题子路径声明模块形状。
declare module "prismjs" {
  const Prism: {
    languages: Record<string, unknown>;
    highlight: (text: string, grammar: unknown, language: string) => string;
  };
  export default Prism;
}

declare module "prismjs/components/prism-json";
declare module "prismjs/components/prism-toml";
declare module "prismjs/themes/prism-tomorrow.css";
