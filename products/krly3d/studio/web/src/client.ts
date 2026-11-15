import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { StudioService } from "../../gen/krly/studio/v1/studio_pb.ts";
import type { CadResponse, Construction } from "../../gen/krly/studio/v1/studio_pb.ts";

const client=createClient(StudioService,createGrpcWebTransport({baseUrl:window.location.origin}));
export type MeshData={positions:number[];indices:number[];volume:number;area:number;openEdges:number;source:string;units?:string};
export type ExpertQuestionData={id:string;title:string;reason:string;options:{id:string;label:string;description:string}[]};
export type UsageCall={id:string;stage:string;model:string;inputTokens:number;outputTokens:number;cachedTokens:number;totalTokens:number;costUsd:number;costKind:'reported'|'estimated'|'unknown';tokensKnown:boolean;searchCalls:number};
export type Usage={calls:UsageCall[]};
export type Source={title:string;url:string};
export type StudioEvent={runId:string;seq:number;kind:"status"|"message"|"mesh"|"artifact"|"completed"|"error"|"question"|"usage";phase:string;text:string;mesh?:MeshData;construction?:Construction;artifact?:{id:string;name:string;mime:string;size:number};elapsedMs:number;question?:ExpertQuestionData;usage?:Usage;sources:Source[]};
export type Health={chat:boolean;vision:boolean;speech:boolean;embedding:boolean;model:string;native:boolean;nativeCharacter:boolean;message:string};
export type RunInput={sessionId:string;text:string;attachmentIds:string[];mode:"chat"|"build"|"demo"|"complex"|"character";height:number;pixelUnit:number;tolerance:number;seed?:[number,number];questionId?:string;answerIds?:string[]};
export async function health(signal?:AbortSignal):Promise<Health> { return await client.health({},{signal,timeoutMs:5000}); }
export async function upload(file:File,signal?:AbortSignal) {return await client.upload({name:file.name,mime:file.type,data:new Uint8Array(await file.arrayBuffer())},{signal,timeoutMs:30_000});}
export async function* run(input:RunInput,signal:AbortSignal):AsyncGenerator<StudioEvent> {
  for await (const event of client.run({...input,seed:input.seed??[]},{signal,timeoutMs:120_000})) yield {...event,kind:event.kind as StudioEvent["kind"],usage:event.usage ? {calls:event.usage.calls.map(call => ({...call,costKind:call.costKind === 'reported' || call.costKind === 'estimated' ? call.costKind : 'unknown'}))} : undefined};
}
export async function transcribe(blob:Blob,signal?:AbortSignal):Promise<string> {return (await client.transcribe({data:new Uint8Array(await blob.arrayBuffer()),mime:blob.type},{signal,timeoutMs:60_000})).text;}
export async function download(id:string,signal?:AbortSignal) {const data=await client.download({id},{signal,timeoutMs:30_000});return {bytes:data.data,name:data.name,mime:data.mime};}
export type CadInput = { sessionId:string; action:'start'|'review'|'show'|'undo'|'redo'; source?:string; modelId?:string; baseRevision?:number };
export type CadState = CadResponse;
export async function cad(input:CadInput,signal?:AbortSignal):Promise<CadState> {
  return await client.cad({sessionId:input.sessionId,action:input.action,source:input.source??'',modelId:input.modelId??'',baseRevision:input.baseRevision??0},{signal,timeoutMs:120_000});
}
