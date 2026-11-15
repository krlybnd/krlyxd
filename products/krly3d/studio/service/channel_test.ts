import { Channel } from "./channel.ts";
Deno.test("slow RPC consumer backpressures graph producer and cancellation releases it", async () => {
  const channel = new Channel<number>(1);
  await channel.send(1);
  let second = false;
  const pending = channel.send(2).then(() => {
    second = true;
  }, () => {});
  await Promise.resolve();
  if (second) throw new Error("producer outran capacity");
  channel.close(new Error("cancelled"));
  await pending;
  if (second) throw new Error("accepted a write after cancellation");
});
Deno.test("successful graph drain preserves order and terminates", async () => {
  const channel = new Channel<number>(2);
  const producer = (async () => {
    for (let i = 0; i < 10; i++) await channel.send(i);
    channel.close();
  })();
  const received: number[] = [];
  for await (const value of channel) received.push(value);
  await producer;
  if (received.join() !== "0,1,2,3,4,5,6,7,8,9") {
    throw new Error("incorrect event order");
  }
});
