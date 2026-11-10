import { secret } from "missing-server-implementation";

globalThis.SERVER_IMPLEMENTATION_MUST_NOT_REACH_BROWSER = secret;

export class Greeting {
  async greet() {
    return secret;
  }
}
export default Greeting;
