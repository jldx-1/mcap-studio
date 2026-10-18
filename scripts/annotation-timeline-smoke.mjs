import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import ts from "../frontend/node_modules/typescript/lib/typescript.js";
const {outputText}=ts.transpileModule(readFileSync(new URL("../frontend/src/utils/annotationTimeline.ts",import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
const {annotationTimeline,submissionSummary}=await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
const box={class_id:0,track_id:1,x:1,y:1,width:2,height:2};
const state={annotations:{"1":{
 "0":{boxes:[box],labels:[]},"1":{boxes:[box],labels:[]},"2":{boxes:[],labels:[],explicit_empty:true,reviewed:true},"3":{boxes:[box],labels:[0],skipped:true},"4":{boxes:[],labels:[1]},"5":{boxes:[],labels:[],reviewed:true},"20":{boxes:[box],labels:[]}
}},tracks:{"1":{"1":{start:0,end:null,keyframes:{"0":box}}}}};
await test("timeline uses actual coverage, preserves legacy flags and clamps track spans",()=>{
 const value=annotationTimeline(state,"1",6);
 assert.deepEqual(value.coverage,[{start:0,end:1,kind:"labeled"},{start:2,end:2,kind:"empty"},{start:3,end:3,kind:"skipped"},{start:4,end:4,kind:"labeled"},{start:5,end:5,kind:"complete"}]);
 assert.deepEqual(value.tracks,[{id:1,start:0,end:5,classId:0}]);
 assert.deepEqual(annotationTimeline(state,"missing",6),{coverage:[],tracks:[]});
});
await test("submission summaries merge overlapping ranges and exclude skipped or out-of-range annotations",()=>{
 assert.deepEqual(submissionSummary(state,"1",6,[{start_frame:0,end_frame:3},{start_frame:1,end_frame:99}]),{total:6,labeled:3,boxes:2,classes:2});
 assert.deepEqual(submissionSummary(state,"1",6,[{start_frame:2,end_frame:3}]),{total:2,labeled:0,boxes:0,classes:0});
});
