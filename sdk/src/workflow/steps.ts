import { StepOptions, Context } from './types';

export interface StepMethods {
  step(
    name: string,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): this;

  action(
    name: string,
    handlerFn: (ctx: Context) => any | Promise<any>,
    options?: StepOptions
  ): this;

  retry(options: {
    attempts: number;
    backoff: { strategy: 'exponential' | 'fixed'; delay: string | number };
  }): this;

  timeout(duration: string | number): this;

  cache(config: { key: (ctx: Context) => string; ttl: string }): this;

  delay(duration: string | number): this;

  onError(handlerFn: (ctx: Context) => any): this;

  log(
    message: string | ((ctx: Context) => string),
    level?: 'info' | 'warn' | 'error'
  ): this;

  sleep(duration: string | number): this;
}
