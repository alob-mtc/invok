pub const PACKAGE_JSON_TEMPLATE: &str = r#"
{
  "name": "serverless-function",
  "scripts": {
    "build": "tsc"
  },
  "engines": {
    "node": "22"
  },
  "main": "lib/index.js",
  "dependencies": {
    // TODO: express
  },
  "devDependencies": {
    "@typescript-eslint/eslint-plugin": "^5.12.0",
    "@typescript-eslint/parser": "^5.12.0",
    "eslint": "^8.9.0",
    "eslint-config-google": "^0.14.0",
    "eslint-plugin-import": "^2.25.4",
    "typescript": "^5.7.3"
  },
  "private": true
}
"#;

pub const TS_CONFIG_TEMPLATE: &str = r#"
{
  "compilerOptions": {
    "module": "NodeNext",
    "esModuleInterop": true,
    "moduleResolution": "nodenext",
    "noImplicitReturns": true,
    "noUnusedLocals": true,
    "outDir": "lib",
    "sourceMap": true,
    "strict": true,
    "target": "es2017"
  },
  "compileOnSave": true,
  "include": [
    "src"
  ]
}
"#;

pub const DOCKERFILE_TEMPLATE: &str = r#"
"#;
