import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "../frontend/node_modules/typescript/lib/typescript.js";
const {outputText}=ts.transpileModule(readFileSync(new URL("../frontend/src/utils/library.ts",import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
const {coveredFrames,libraryWorkflow,recordingFamilies}=await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
const scope=(start,end,channel="1")=>({channel_id:channel,start_frame:start,end_frame:end});
const recording={uuid:"r",summary:{annotation_topics:{"1":{required:true,frame_count:10,annotated_count:0},"2":{required:true,frame_count:5,annotated_count:0}}}};
const task=(status,scopes,extra={})=>({recording_id:"r",status,scopes,annotated_count:0,working_dirty:false,...extra});
await test("approval merges overlapping ranges and requires all required topics",()=>{
 assert.equal(coveredFrames([scope(-1,3),scope(2,6),scope(9,20),scope(0,20,"2")],"1",10),8);
 assert.equal(libraryWorkflow(recording,[]).status,"Needs labeling");
 const partial=[task("approved",[scope(0,5)]),task("approved",[scope(3,9)])];
 assert.equal(libraryWorkflow(recording,partial).approved,10);
 assert.equal(libraryWorkflow(recording,partial).status,"Partially approved");
 const all=[...partial,task("approved",[scope(0,4,"2")])];
 assert.equal(libraryWorkflow(recording,all).status,"Approved");
 assert.equal(libraryWorkflow(recording,[...all,task("submitted",[scope(0,2)])]).status,"In review");
 assert.equal(libraryWorkflow(recording,[...all,task("changes_requested",[scope(0,2)])]).status,"Changes requested");
 assert.equal(libraryWorkflow(recording,all.map(value=>({...value,working_dirty:true}))).status,"Labeling");
 assert.equal(libraryWorkflow({...recording,dataset_usages:[{dataset_id:"published"}]},all).status,"Approved");
});
await test("structured inputs remain imported until labeling starts; derivatives share one inventory row",()=>{
 assert.equal(libraryWorkflow({uuid:"r",topic_details:[{channel_id:"1",modality:"IMU",message_count:10}]},[]).status,"Imported");
 const source={uuid:"source",created_at:"2020-01-01"},derived={uuid:"derived",source_recording_uuid:"source",created_at:"2021-01-01"};
 const groups=recordingFamilies([source,derived]);assert.equal(groups.length,1);assert.equal(groups[0].item.uuid,"derived");assert.equal(groups[0].members.length,2);
});
